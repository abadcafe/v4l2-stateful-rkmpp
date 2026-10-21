//! module: quantize::device; responsibility: own a hardware Vulkan
//! device. operations: new, api, family, queue, memory_bits,
//! export_fence; entities: Device, Instance; tests: device_tests,
//! context_tests.
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use ash::{Entry, vk};
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
  family: u32,
  queue: Mutex<vk::Queue>,
  external: ash::khr::external_memory_fd::Device,
}
fn error(operation: &str, result: vk::Result) -> io::Error {
  io::Error::other(format!("Vulkan {operation}: {result:?}"))
}
impl Device {
  pub(super) fn new() -> io::Result<Arc<Self>> {
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
      ash::khr::external_fence_fd::NAME,
      ash::ext::external_memory_dma_buf::NAME,
      ash::ext::queue_family_foreign::NAME,
    ];
    for name in names {
      if !available.iter().any(|p| {
        p.extension_name_as_c_str()
          .is_ok_and(|available| available == name)
      }) {
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
      family,
      queue: Mutex::new(queue),
      external,
    }))
  }

  pub(super) fn family(&self) -> u32 {
    self.family
  }

  /// # Safety
  /// Commands and fence belong to this device and retain their
  /// resources until completion. Each command buffer and fence has
  /// one submission owner.
  pub(super) unsafe fn submit(
    &self,
    commands: &[vk::CommandBuffer],
    fence: vk::Fence,
  ) -> io::Result<()> {
    let queue = self.queue.lock().unwrap();
    // SAFETY: the caller owns the resources; the mutex serializes the
    // shared queue.
    unsafe {
      self.api.queue_submit(
        *queue,
        &[vk::SubmitInfo::default().command_buffers(commands)],
        fence,
      )
    }
    .map_err(|r| error("queue submit", r))
  }

  /// # Safety
  /// Fence belongs to this device, supports SYNC_FD export, and has a
  /// pending or completed submission; its payload is exported once.
  pub(super) unsafe fn export_fence(
    &self,
    fence: vk::Fence,
  ) -> io::Result<Option<OwnedFd>> {
    let external = ash::khr::external_fence_fd::Device::new(
      &self.instance.0,
      &self.api,
    );
    let fd = unsafe {
      external.get_fence_fd(
        &vk::FenceGetFdInfoKHR::default()
          .fence(fence)
          .handle_type(vk::ExternalFenceHandleTypeFlags::SYNC_FD),
      )
    }
    .map_err(|r| error("export completion fence", r))?;
    Ok(if fd == -1 {
      None
    } else {
      Some(unsafe { OwnedFd::from_raw_fd(fd) })
    })
  }

  pub(super) fn api(&self) -> &ash::Device {
    &self.api
  }

  pub(super) fn memory_bits(
    &self,
    fd: BorrowedFd<'_>,
  ) -> io::Result<u32> {
    let mut properties = vk::MemoryFdPropertiesKHR::default();
    unsafe {
      self.external.get_memory_fd_properties(
        vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
        fd.as_raw_fd(),
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
  }
}
