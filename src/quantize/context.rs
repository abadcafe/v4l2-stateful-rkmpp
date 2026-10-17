//! module: quantize::context; responsibility: own a Vulkan compute
//! execution context. operations: new, device, quiesce, dispatch;
//! entities: Device, Context. tests: context_tests and quantize_tests
//! through the public conversion interface.
use std::io;
use std::sync::Arc;

use ash::{Entry, vk};

use super::layout::Layout;
struct Instance(ash::Instance);
impl Drop for Instance {
  fn drop(&mut self) {
    unsafe {
      self.0.destroy_instance(None);
    }
  }
}
pub(super) struct Device {
  instance: Arc<Instance>,
  api: ash::Device,
  physical: vk::PhysicalDevice,
  family: u32,
  queue: vk::Queue,
  external: ash::khr::external_memory_fd::Device,
}
fn error(operation: &str, result: vk::Result) -> io::Error {
  io::Error::other(format!("Vulkan {operation}: {result:?}"))
}
impl Device {
  fn new() -> io::Result<Arc<Self>> {
    let entry = Entry::linked();
    let app = vk::ApplicationInfo::default()
      .application_name(c"v4l2-rkmpp-quantize")
      .api_version(vk::API_VERSION_1_1);
    let instance = Arc::new(Instance(
      unsafe {
        entry.create_instance(
          &vk::InstanceCreateInfo::default().application_info(&app),
          None,
        )
      }
      .map_err(|r| error("create instance", r))?,
    ));
    let mut selected = None;
    for physical in unsafe { instance.0.enumerate_physical_devices() }
      .map_err(|r| error("enumerate devices", r))?
    {
      let properties =
        unsafe { instance.0.get_physical_device_properties(physical) };
      if properties.device_type == vk::PhysicalDeviceType::CPU {
        continue;
      }
      let families = unsafe {
        instance.0.get_physical_device_queue_family_properties(physical)
      };
      if let Some(family) = families
        .iter()
        .position(|p| p.queue_flags.contains(vk::QueueFlags::COMPUTE))
      {
        selected = Some((physical, family as u32));
        break;
      }
    }
    let (physical, family) = selected.ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::NotFound,
        "no hardware Vulkan compute device",
      )
    })?;
    let available = unsafe {
      instance.0.enumerate_device_extension_properties(physical)
    }
    .map_err(|r| error("enumerate extensions", r))?;
    let names = [
      ash::khr::external_memory_fd::NAME,
      ash::ext::external_memory_dma_buf::NAME,
      ash::ext::queue_family_foreign::NAME,
    ];
    for name in names {
      if !available
                .iter()
                .any(|p| unsafe { std::ffi::CStr::from_ptr(p.extension_name.as_ptr()) } == name)
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("missing Vulkan extension {name:?}"),
                ));
            }
    }
    let priorities = [0.5];
    let queues = [vk::DeviceQueueCreateInfo::default()
      .queue_family_index(family)
      .queue_priorities(&priorities)];
    let names = names.map(|n| n.as_ptr());
    let info = vk::DeviceCreateInfo::default()
      .queue_create_infos(&queues)
      .enabled_extension_names(&names);
    let api =
      unsafe { instance.0.create_device(physical, &info, None) }
        .map_err(|r| error("create device", r))?;
    let queue = unsafe { api.get_device_queue(family, 0) };
    let external =
      ash::khr::external_memory_fd::Device::new(&instance.0, &api);
    Ok(Arc::new(Self {
      instance,
      api,
      physical,
      family,
      queue,
      external,
    }))
  }

  pub(super) fn api(&self) -> &ash::Device {
    &self.api
  }

  pub(super) fn memory_bits(&self, fd: i32) -> io::Result<u32> {
    let mut properties = vk::MemoryFdPropertiesKHR::default();
    unsafe {
      self.external.get_memory_fd_properties(
        vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
        fd,
        &mut properties,
      )
    }
    .map_err(|r| error("DMA-BUF properties", r))?;
    Ok(properties.memory_type_bits)
  }
}
impl Drop for Device {
  fn drop(&mut self) {
    unsafe {
      let _ = self.api.device_wait_idle();
      self.api.destroy_device(None);
    }
    // Keep the instance lease until after device destruction.
    let _ = (&self.instance, self.physical);
  }
}
pub(super) struct Context {
  gpu: Arc<Device>,
  descriptor_layout: vk::DescriptorSetLayout,
  layout: vk::PipelineLayout,
  pipeline: vk::Pipeline,
  descriptors_pool: vk::DescriptorPool,
  descriptors: vk::DescriptorSet,
  pool: vk::CommandPool,
  commands: vk::CommandBuffer,
  fence: vk::Fence,
}
impl Context {
  pub(super) fn new() -> io::Result<Self> {
    let mut context = Self {
      gpu: Device::new()?,
      descriptor_layout: vk::DescriptorSetLayout::null(),
      layout: vk::PipelineLayout::null(),
      pipeline: vk::Pipeline::null(),
      descriptors_pool: vk::DescriptorPool::null(),
      descriptors: vk::DescriptorSet::null(),
      pool: vk::CommandPool::null(),
      commands: vk::CommandBuffer::null(),
      fence: vk::Fence::null(),
    };
    context.initialize()?;
    Ok(context)
  }

  fn initialize(&mut self) -> io::Result<()> {
    // SAFETY: all handles belong to gpu; local descriptor slices
    // outlive their calls.
    unsafe {
      let d = self.gpu.api();
      let bindings = [0, 1].map(|binding| {
        vk::DescriptorSetLayoutBinding::default()
          .binding(binding)
          .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
          .descriptor_count(1)
          .stage_flags(vk::ShaderStageFlags::COMPUTE)
      });
      self.descriptor_layout = d
        .create_descriptor_set_layout(
          &vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(&bindings),
          None,
        )
        .map_err(|r| error("descriptor layout", r))?;
      let layouts = [self.descriptor_layout];
      let pushes = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .size(28)];
      self.layout = d
        .create_pipeline_layout(
          &vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&layouts)
            .push_constant_ranges(&pushes),
          None,
        )
        .map_err(|r| error("pipeline layout", r))?;
      let shader =
        include_bytes!(concat!(env!("OUT_DIR"), "/quantize.spv"));
      let words: Vec<u32> = shader
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
      let module = d
        .create_shader_module(
          &vk::ShaderModuleCreateInfo::default().code(&words),
          None,
        )
        .map_err(|r| error("shader module", r))?;
      let stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(module)
        .name(c"main");
      let info = [vk::ComputePipelineCreateInfo::default()
        .stage(stage)
        .layout(self.layout)];
      let pipelines = d.create_compute_pipelines(
        vk::PipelineCache::null(),
        &info,
        None,
      );
      d.destroy_shader_module(module, None);
      self.pipeline = match pipelines {
        Ok(p) => p[0],
        Err((partial, result)) => {
          for pipeline in partial {
            d.destroy_pipeline(pipeline, None);
          }
          return Err(error("compute pipeline", result));
        }
      };
      let sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(2)];
      self.descriptors_pool = d
        .create_descriptor_pool(
          &vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&sizes),
          None,
        )
        .map_err(|r| error("descriptor pool", r))?;
      self.descriptors = d
        .allocate_descriptor_sets(
          &vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptors_pool)
            .set_layouts(&layouts),
        )
        .map_err(|r| error("allocate descriptors", r))?[0];
      self.pool = d
        .create_command_pool(
          &vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(self.gpu.family),
          None,
        )
        .map_err(|r| error("command pool", r))?;
      self.commands = d
        .allocate_command_buffers(
          &vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1),
        )
        .map_err(|r| error("command buffer", r))?[0];
      self.fence = d
        .create_fence(&vk::FenceCreateInfo::default(), None)
        .map_err(|r| error("fence", r))?;
    }
    Ok(())
  }

  pub(super) fn device(&self) -> Arc<Device> {
    self.gpu.clone()
  }

  pub(super) fn quiesce(&self) -> io::Result<()> {
    // An error completion must not release its input frame while
    // submitted GPU work still owns it.
    unsafe { self.gpu.api().device_wait_idle() }
      .map_err(|r| error("drain failed dispatch", r))
  }

  pub(super) fn dispatch(
    &mut self,
    buffers: [vk::DescriptorBufferInfo; 2],
    layout: Layout,
  ) -> io::Result<()> {
    if buffers.iter().any(|b| b.buffer == vk::Buffer::null())
      || buffers[0].buffer == buffers[1].buffer
      || buffers[0].range < layout.source_size()
      || buffers[1].range < layout.destination_size()
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid conversion descriptors",
      ));
    }
    let parameters = layout.parameters();
    // SAFETY: imports remain alive until this synchronous fence
    // completes; only this owner records/submits.
    unsafe {
      let d = self.gpu.api();
      let writes = [0, 1].map(|i| {
        vk::WriteDescriptorSet::default()
          .dst_set(self.descriptors)
          .dst_binding(i as u32)
          .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
          .buffer_info(&buffers[i..i + 1])
      });
      d.update_descriptor_sets(&writes, &[]);
      d.reset_command_buffer(
        self.commands,
        vk::CommandBufferResetFlags::empty(),
      )
      .map_err(|r| error("reset commands", r))?;
      d.begin_command_buffer(
        self.commands,
        &vk::CommandBufferBeginInfo::default()
          .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
      )
      .map_err(|r| error("begin commands", r))?;
      let mut barriers = [0, 1].map(|i| {
        vk::BufferMemoryBarrier::default()
          .dst_access_mask(if i == 0 {
            vk::AccessFlags::SHADER_READ
          } else {
            vk::AccessFlags::SHADER_WRITE
          })
          .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
          .dst_queue_family_index(self.gpu.family)
          .buffer(buffers[i].buffer)
          .size(vk::WHOLE_SIZE)
      });
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &barriers,
        &[],
      );
      d.cmd_bind_pipeline(
        self.commands,
        vk::PipelineBindPoint::COMPUTE,
        self.pipeline,
      );
      d.cmd_bind_descriptor_sets(
        self.commands,
        vk::PipelineBindPoint::COMPUTE,
        self.layout,
        0,
        &[self.descriptors],
        &[],
      );
      let bytes = std::slice::from_raw_parts(
        parameters.as_ptr().cast::<u8>(),
        28,
      );
      d.cmd_push_constants(
        self.commands,
        self.layout,
        vk::ShaderStageFlags::COMPUTE,
        0,
        bytes,
      );
      d.cmd_dispatch(
        self.commands,
        parameters[0].div_ceil(256),
        parameters[1] + parameters[1].div_ceil(2),
        1,
      );
      // MMAP/CPU consumers need a device-to-host domain operation as
      // well as the foreign queue ownership release. Waiting for the
      // dispatch fence alone does not make shader writes host-visible.
      let host = [vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::HOST_READ)];
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::HOST,
        vk::DependencyFlags::empty(),
        &host,
        &[],
        &[],
      );
      for barrier in &mut barriers {
        barrier.src_access_mask = barrier.dst_access_mask;
        barrier.dst_access_mask = vk::AccessFlags::empty();
        barrier.src_queue_family_index = self.gpu.family;
        barrier.dst_queue_family_index = vk::QUEUE_FAMILY_FOREIGN_EXT;
      }
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        vk::DependencyFlags::empty(),
        &[],
        &barriers,
        &[],
      );
      d.end_command_buffer(self.commands)
        .map_err(|r| error("end commands", r))?;
      d.reset_fences(&[self.fence])
        .map_err(|r| error("reset fence", r))?;
      let commands = [self.commands];
      d.queue_submit(
        self.gpu.queue,
        &[vk::SubmitInfo::default().command_buffers(&commands)],
        self.fence,
      )
      .map_err(|r| error("queue submit", r))?;
      d.wait_for_fences(&[self.fence], true, 2_000_000_000)
        .map_err(|r| error("wait fence", r))?;
    }
    Ok(())
  }
}
impl Drop for Context {
  fn drop(&mut self) {
    unsafe {
      let d = self.gpu.api();
      let _ = d.device_wait_idle();
      d.destroy_fence(self.fence, None);
      d.destroy_command_pool(self.pool, None);
      d.destroy_descriptor_pool(self.descriptors_pool, None);
      d.destroy_pipeline(self.pipeline, None);
      d.destroy_pipeline_layout(self.layout, None);
      d.destroy_descriptor_set_layout(self.descriptor_layout, None);
    }
  }
}
