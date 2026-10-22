//! module: quantize::context; responsibility: own a Vulkan compute
//! execution context. operations: new, device, quiesce, dispatch,
//! completion, finish; entities: Context, Submission. tests:
//! context_tests and quantize_tests through the public conversion
//! interface.
use std::io;
use std::sync::Arc;

use ash::vk;

use super::device::Device;
use super::imports::Reservation;
use super::layout::Layout;
use super::pipeline::{Factory, Pipeline};
use crate::fence::Fence;
fn error(operation: &str, result: vk::Result) -> io::Error {
  io::Error::other(format!("Vulkan {operation}: {result:?}"))
}
struct Submission {
  _imports: Reservation,
  state: Submitted,
}
enum Submitted {
  Preparing,
  Queued,
  Exported(Fence),
}
pub(super) struct Context {
  gpu: Arc<Device>,
  program: Arc<Pipeline>,
  descriptors_pool: vk::DescriptorPool,
  descriptors: vk::DescriptorSet,
  pool: vk::CommandPool,
  commands: vk::CommandBuffer,
  fence: vk::Fence,
  pending: Option<Submission>,
  failed: bool,
}
impl Context {
  pub(super) fn new(factory: &Factory) -> io::Result<Self> {
    let program = factory.get()?;
    let mut context = Self {
      gpu: program.device(),
      program,
      descriptors_pool: vk::DescriptorPool::null(),
      descriptors: vk::DescriptorSet::null(),
      pool: vk::CommandPool::null(),
      commands: vk::CommandBuffer::null(),
      fence: vk::Fence::null(),
      pending: None,
      failed: false,
    };
    context.initialize()?;
    Ok(context)
  }

  fn initialize(&mut self) -> io::Result<()> {
    // SAFETY: all handles belong to gpu; local descriptor slices
    // outlive their calls.
    let d = self.gpu.api();
    let layouts = [self.program.descriptor_layout()];
    let sizes = [vk::DescriptorPoolSize::default()
      .ty(vk::DescriptorType::STORAGE_BUFFER)
      .descriptor_count(2)];
    self.descriptors_pool = unsafe {
      d.create_descriptor_pool(
        &vk::DescriptorPoolCreateInfo::default()
          .max_sets(1)
          .pool_sizes(&sizes),
        None,
      )
    }
    .map_err(|r| error("descriptor pool", r))?;
    self.descriptors = unsafe {
      d.allocate_descriptor_sets(
        &vk::DescriptorSetAllocateInfo::default()
          .descriptor_pool(self.descriptors_pool)
          .set_layouts(&layouts),
      )
    }
    .map_err(|r| error("allocate descriptors", r))?[0];
    self.pool = unsafe {
      d.create_command_pool(
        &vk::CommandPoolCreateInfo::default()
          .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
          .queue_family_index(self.gpu.family()),
        None,
      )
    }
    .map_err(|r| error("command pool", r))?;
    self.commands = unsafe {
      d.allocate_command_buffers(
        &vk::CommandBufferAllocateInfo::default()
          .command_pool(self.pool)
          .level(vk::CommandBufferLevel::PRIMARY)
          .command_buffer_count(1),
      )
    }
    .map_err(|r| error("command buffer", r))?[0];
    let mut export = vk::ExportFenceCreateInfo::default()
      .handle_types(vk::ExternalFenceHandleTypeFlags::SYNC_FD);
    self.fence = unsafe {
      d.create_fence(
        &vk::FenceCreateInfo::default().push_next(&mut export),
        None,
      )
    }
    .map_err(|r| error("fence", r))?;
    Ok(())
  }

  pub(super) fn device(&self) -> Arc<Device> {
    self.gpu.clone()
  }

  pub(super) fn quiesce(&self) -> io::Result<()> {
    // An error completion must not release its input frame while
    // submitted GPU work still owns it.
    match self.pending.as_ref().map(|pending| &pending.state) {
      Some(Submitted::Exported(fence)) => fence.wait(),
      Some(Submitted::Queued) => {
        // SAFETY: export did not consume the submitted fence payload.
        unsafe {
          self.gpu.api().wait_for_fences(&[self.fence], true, u64::MAX)
        }
        .map_err(|r| error("drain dispatch", r))
      }
      Some(Submitted::Preparing) | None => Ok(()),
    }
  }

  pub(super) fn completion(&self) -> Option<crate::descriptor::Lease> {
    self
      .pending
      .as_ref()
      .and_then(|p| {
        if let Submitted::Exported(fence) = &p.state {
          Some(fence)
        } else {
          None
        }
      })
      .and_then(Fence::lease)
  }

  pub(super) fn finish(&mut self) -> io::Result<bool> {
    if self.failed {
      return Err(io::Error::other("Vulkan context failed previously"));
    }
    if let Some(fence) = self.pending.as_ref().and_then(|p| {
      if let Submitted::Exported(fence) = &p.state {
        Some(fence)
      } else {
        None
      }
    }) {
      match fence.ready() {
        Ok(false) => return Ok(false),
        Ok(true) => {}
        Err(error) => {
          self.failed = true;
          return Err(error);
        }
      }
    }
    self.pending = None;
    Ok(true)
  }

  pub(super) fn dispatch(
    &mut self,
    imports: Reservation,
    layout: Layout,
  ) -> io::Result<()> {
    if self.failed {
      return Err(io::Error::other("Vulkan context failed previously"));
    }
    if self.pending.is_some() {
      return Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "Vulkan submission has not been retired",
      ));
    }
    let buffers = imports.descriptors(&self.gpu)?;
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
    // Any failed Vulkan call latches the context. No later dispatch
    // can recycle resources whose submission status is uncertain.
    self.failed = true;
    self.pending = Some(Submission {
      _imports: imports,
      state: Submitted::Preparing,
    });
    // SAFETY: this submission owns its imports until completion or
    // quiesce; this context exclusively owns command resource
    // retirement.
    let d = self.gpu.api();
    let writes = [0, 1].map(|i| {
      vk::WriteDescriptorSet::default()
        .dst_set(self.descriptors)
        .dst_binding(i as u32)
        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
        .buffer_info(&buffers[i..i + 1])
    });
    unsafe { d.update_descriptor_sets(&writes, &[]) };
    unsafe {
      d.reset_command_buffer(
        self.commands,
        vk::CommandBufferResetFlags::empty(),
      )
    }
    .map_err(|r| error("reset commands", r))?;
    unsafe {
      d.begin_command_buffer(
        self.commands,
        &vk::CommandBufferBeginInfo::default()
          .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
      )
    }
    .map_err(|r| error("begin commands", r))?;
    let mut barriers = [0, 1].map(|i| {
      vk::BufferMemoryBarrier::default()
        .dst_access_mask(if i == 0 {
          vk::AccessFlags::SHADER_READ
        } else {
          vk::AccessFlags::SHADER_WRITE
        })
        .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
        .dst_queue_family_index(self.gpu.family())
        .buffer(buffers[i].buffer)
        .size(vk::WHOLE_SIZE)
    });
    unsafe {
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &barriers,
        &[],
      )
    };
    unsafe {
      d.cmd_bind_pipeline(
        self.commands,
        vk::PipelineBindPoint::COMPUTE,
        self.program.handle(),
      )
    };
    unsafe {
      d.cmd_bind_descriptor_sets(
        self.commands,
        vk::PipelineBindPoint::COMPUTE,
        self.program.layout(),
        0,
        &[self.descriptors],
        &[],
      )
    };
    let encoded = parameters.map(u32::to_ne_bytes);
    let bytes = encoded.as_flattened();
    unsafe {
      d.cmd_push_constants(
        self.commands,
        self.program.layout(),
        vk::ShaderStageFlags::COMPUTE,
        0,
        bytes,
      )
    };
    unsafe {
      d.cmd_dispatch(
        self.commands,
        parameters[0].div_ceil(256),
        parameters[1] + parameters[1].div_ceil(2),
        1,
      )
    };
    // MMAP/CPU consumers need a device-to-host domain operation as
    // well as the foreign queue ownership release. Waiting for the
    // dispatch fence alone does not make shader writes host-visible.
    let host = [vk::MemoryBarrier::default()
      .src_access_mask(vk::AccessFlags::SHADER_WRITE)
      .dst_access_mask(vk::AccessFlags::HOST_READ)];
    unsafe {
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::HOST,
        vk::DependencyFlags::empty(),
        &host,
        &[],
        &[],
      )
    };
    for barrier in &mut barriers {
      barrier.src_access_mask = barrier.dst_access_mask;
      barrier.dst_access_mask = vk::AccessFlags::empty();
      barrier.src_queue_family_index = self.gpu.family();
      barrier.dst_queue_family_index = vk::QUEUE_FAMILY_FOREIGN_EXT;
    }
    unsafe {
      d.cmd_pipeline_barrier(
        self.commands,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        vk::DependencyFlags::empty(),
        &[],
        &barriers,
        &[],
      )
    };
    unsafe { d.end_command_buffer(self.commands) }
      .map_err(|r| error("end commands", r))?;
    unsafe { d.reset_fences(&[self.fence]) }
      .map_err(|r| error("reset fence", r))?;
    let commands = [self.commands];
    unsafe { self.gpu.submit(&commands, self.fence) }?;
    self.pending.as_mut().unwrap().state = Submitted::Queued;
    let fence =
      Fence::new(unsafe { self.gpu.export_fence(self.fence) }?);
    self.pending.as_mut().unwrap().state = Submitted::Exported(fence);
    self.failed = false;
    Ok(())
  }
}
impl Drop for Context {
  fn drop(&mut self) {
    let _ = self.quiesce();
    let d = self.gpu.api();
    unsafe { d.destroy_fence(self.fence, None) };
    unsafe { d.destroy_command_pool(self.pool, None) };
    unsafe { d.destroy_descriptor_pool(self.descriptors_pool, None) };
  }
}
