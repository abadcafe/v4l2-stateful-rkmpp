//! module: quantize::imports; responsibility: cache owned Vulkan
//! DMA-BUF imports. operations: new, pair; entities: Cache; tests:
//! imports_tests, quantize_tests.
use std::io;
use std::num::NonZeroUsize;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::Arc;

use ash::vk;

use super::context::Device;
struct Imported {
  gpu: Arc<Device>,
  buffer: vk::Buffer,
  memory: vk::DeviceMemory,
  identity: (u64, u64),
  size: u64,
  age: u64,
}
impl Imported {
  fn new(
    gpu: Arc<Device>,
    fd: BorrowedFd<'_>,
    stat: &libc::stat,
    age: u64,
  ) -> io::Result<Self> {
    if stat.st_size <= 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "empty DMA-BUF import",
      ));
    }
    let mut imported = Self {
      gpu,
      buffer: vk::Buffer::null(),
      memory: vk::DeviceMemory::null(),
      identity: (stat.st_dev, stat.st_ino),
      size: stat.st_size as u64,
      age,
    };
    unsafe {
      let d = imported.gpu.api();
      let mut external = vk::ExternalMemoryBufferCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
      let info = vk::BufferCreateInfo::default()
        .push_next(&mut external)
        .size(imported.size)
        .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
      imported.buffer = d.create_buffer(&info, None).map_err(|r| {
        io::Error::other(format!("Vulkan create import: {r:?}"))
      })?;
      let requirements =
        d.get_buffer_memory_requirements(imported.buffer);
      if requirements.size > imported.size {
        return Err(io::Error::new(
          io::ErrorKind::InvalidInput,
          "Vulkan allocation exceeds DMA-BUF",
        ));
      }
      let bits = requirements.memory_type_bits
        & imported.gpu.memory_bits(fd.as_raw_fd())?;
      if bits == 0 {
        return Err(io::Error::new(
          io::ErrorKind::Unsupported,
          "no compatible DMA-BUF memory type",
        ));
      }
      let raw = libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
      if raw < 0 {
        return Err(io::Error::last_os_error());
      }
      let duplicate = OwnedFd::from_raw_fd(raw);
      let mut external = vk::ImportMemoryFdInfoKHR::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
        .fd(raw);
      let info = vk::MemoryAllocateInfo::default()
        .push_next(&mut external)
        .allocation_size(requirements.size)
        .memory_type_index(bits.trailing_zeros());
      imported.memory =
        d.allocate_memory(&info, None).map_err(|r| {
          io::Error::other(format!("Vulkan import memory: {r:?}"))
        })?;
      // Vulkan takes ownership only after successful allocation.
      let _ = duplicate.into_raw_fd();
      d.bind_buffer_memory(imported.buffer, imported.memory, 0)
        .map_err(|r| {
          io::Error::other(format!("Vulkan bind import: {r:?}"))
        })?;
    }
    Ok(imported)
  }
}
impl Drop for Imported {
  fn drop(&mut self) {
    unsafe {
      self.gpu.api().destroy_buffer(self.buffer, None);
      self.gpu.api().free_memory(self.memory, None);
    }
  }
}
pub(super) struct Cache {
  gpu: Arc<Device>,
  entries: Vec<Imported>,
  clock: u64,
  capacity: usize,
}
impl Cache {
  pub(super) fn new(
    gpu: Arc<Device>,
    capacity: NonZeroUsize,
  ) -> io::Result<Self> {
    if capacity.get() < 2 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "import cache requires two image slots",
      ));
    }
    Ok(Self {
      gpu,
      entries: Vec::with_capacity(capacity.get()),
      clock: 0,
      capacity: capacity.get(),
    })
  }

  pub(super) fn pair(
    &mut self,
    source: BorrowedFd<'_>,
    source_stat: &libc::stat,
    destination: BorrowedFd<'_>,
    destination_stat: &libc::stat,
  ) -> io::Result<[vk::DescriptorBufferInfo; 2]> {
    let input = self.get(source, source_stat, None)?;
    let output =
      self.get(destination, destination_stat, Some(input))?;
    Ok([self.descriptor(input), self.descriptor(output)])
  }

  fn get(
    &mut self,
    fd: BorrowedFd<'_>,
    stat: &libc::stat,
    avoid: Option<usize>,
  ) -> io::Result<usize> {
    self.clock = self.clock.wrapping_add(1);
    let identity = (stat.st_dev, stat.st_ino);
    if let Some(index) =
      self.entries.iter().position(|e| e.identity == identity)
    {
      self.entries[index].age = self.clock;
      return Ok(index);
    }
    let entry = Imported::new(self.gpu.clone(), fd, stat, self.clock)?;
    if self.entries.len() < self.capacity {
      self.entries.push(entry);
      return Ok(self.entries.len() - 1);
    }
    let index = self
      .entries
      .iter()
      .enumerate()
      .filter(|(i, _)| Some(*i) != avoid)
      .min_by_key(|(_, e)| e.age)
      .unwrap()
      .0;
    self.entries[index] = entry;
    Ok(index)
  }

  fn descriptor(&self, index: usize) -> vk::DescriptorBufferInfo {
    let entry = &self.entries[index];
    vk::DescriptorBufferInfo::default()
      .buffer(entry.buffer)
      .range(entry.size)
  }
}
