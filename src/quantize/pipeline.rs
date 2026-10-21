//! module: quantize::pipeline; responsibility: retain reusable GPU
//! compute programs. operations: Factory default/get, Pipeline
//! device/descriptor_layout/layout/handle; entities: Factory, Pipeline;
//! tests: context_tests and quantize_tests.
use std::io;
use std::sync::{Arc, Mutex};

use ash::vk;

use super::device::Device;
fn error(operation: &str, result: vk::Result) -> io::Error {
  io::Error::other(format!("Vulkan {operation}: {result:?}"))
}
#[derive(Clone, Default)]
pub struct Factory(Arc<Mutex<Option<Arc<Pipeline>>>>);
impl Factory {
  pub(super) fn get(&self) -> io::Result<Arc<Pipeline>> {
    let mut shared = self.0.lock().unwrap();
    if let Some(program) = shared.as_ref() {
      return Ok(program.clone());
    }
    let program = Arc::new(Pipeline::new()?);
    *shared = Some(program.clone());
    Ok(program)
  }
}
pub(super) struct Pipeline {
  gpu: Arc<Device>,
  descriptor_layout: vk::DescriptorSetLayout,
  layout: vk::PipelineLayout,
  pipeline: vk::Pipeline,
}
impl Pipeline {
  fn new() -> io::Result<Self> {
    let mut program = Self {
      gpu: Device::new()?,
      descriptor_layout: vk::DescriptorSetLayout::null(),
      layout: vk::PipelineLayout::null(),
      pipeline: vk::Pipeline::null(),
    };
    program.initialize()?;
    Ok(program)
  }

  fn initialize(&mut self) -> io::Result<()> {
    let d = self.gpu.api();
    let bindings = [0, 1].map(|binding| {
      vk::DescriptorSetLayoutBinding::default()
        .binding(binding)
        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
    });
    self.descriptor_layout = unsafe {
      d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default()
          .bindings(&bindings),
        None,
      )
    }
    .map_err(|r| error("descriptor layout", r))?;
    let layouts = [self.descriptor_layout];
    let pushes = [vk::PushConstantRange::default()
      .stage_flags(vk::ShaderStageFlags::COMPUTE)
      .size(28)];
    self.layout = unsafe {
      d.create_pipeline_layout(
        &vk::PipelineLayoutCreateInfo::default()
          .set_layouts(&layouts)
          .push_constant_ranges(&pushes),
        None,
      )
    }
    .map_err(|r| error("pipeline layout", r))?;
    let shader =
      include_bytes!(concat!(env!("OUT_DIR"), "/quantize.spv"));
    let words: Vec<u32> = shader
      .chunks_exact(4)
      .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
      .collect();
    let module = unsafe {
      d.create_shader_module(
        &vk::ShaderModuleCreateInfo::default().code(&words),
        None,
      )
    }
    .map_err(|r| error("shader module", r))?;
    let stage = vk::PipelineShaderStageCreateInfo::default()
      .stage(vk::ShaderStageFlags::COMPUTE)
      .module(module)
      .name(c"main");
    let info = [vk::ComputePipelineCreateInfo::default()
      .stage(stage)
      .layout(self.layout)];
    let pipelines = unsafe {
      d.create_compute_pipelines(vk::PipelineCache::null(), &info, None)
    };
    unsafe { d.destroy_shader_module(module, None) };
    self.pipeline = match pipelines {
      Ok(p) => p[0],
      Err((partial, result)) => {
        for pipeline in partial {
          unsafe { d.destroy_pipeline(pipeline, None) };
        }
        return Err(error("compute pipeline", result));
      }
    };

    Ok(())
  }

  pub(super) fn device(&self) -> Arc<Device> {
    self.gpu.clone()
  }

  pub(super) fn descriptor_layout(&self) -> vk::DescriptorSetLayout {
    self.descriptor_layout
  }

  pub(super) fn layout(&self) -> vk::PipelineLayout {
    self.layout
  }

  pub(super) fn handle(&self) -> vk::Pipeline {
    self.pipeline
  }
}
impl Drop for Pipeline {
  fn drop(&mut self) {
    let d = self.gpu.api();
    // SAFETY: contexts retain this program until their own submissions
    // finish.
    unsafe {
      d.destroy_pipeline(self.pipeline, None);
      d.destroy_pipeline_layout(self.layout, None);
      d.destroy_descriptor_set_layout(self.descriptor_layout, None);
    }
  }
}
