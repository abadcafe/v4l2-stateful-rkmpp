//! module: quantize::imports; responsibility: cache owned Vulkan
//! DMA-BUF imports. operations: new, pair, descriptors; entities:
//! Cache, Pair, Entry, Imported; tests: imports_tests, quantize_tests.
use std::io;
use std::num::NonZeroUsize;
use std::os::fd::{AsRawFd, BorrowedFd, IntoRawFd};
use std::rc::Rc;
use std::sync::Arc;

use ash::vk;

use super::device::Device;
struct Imported {
  gpu: Arc<Device>,
  buffer: vk::Buffer,
  memory: vk::DeviceMemory,
  identity: (u64, u64),
  size: u64,
}
impl Imported {
  fn new(
    gpu: Arc<Device>,
    fd: BorrowedFd<'_>,
    stat: &dma_buf::Metadata,
  ) -> io::Result<Self> {
    if stat.size() <= 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "empty DMA-BUF import",
      ));
    }
    let mut imported = Self {
      gpu,
      buffer: vk::Buffer::null(),
      memory: vk::DeviceMemory::null(),
      identity: stat.identity(),
      size: stat.size() as u64,
    };
    // SAFETY: every Vulkan resource below is owned by imported;
    // descriptor chains borrow local storage only until their call
    // returns.
    let d = imported.gpu.api();
    let mut external = vk::ExternalMemoryBufferCreateInfo::default()
      .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let info = vk::BufferCreateInfo::default()
      .push_next(&mut external)
      .size(imported.size)
      .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
      .sharing_mode(vk::SharingMode::EXCLUSIVE);
    imported.buffer =
      unsafe { d.create_buffer(&info, None) }.map_err(|r| {
        io::Error::other(format!("Vulkan create import: {r:?}"))
      })?;
    let requirements =
      unsafe { d.get_buffer_memory_requirements(imported.buffer) };
    if requirements.size > imported.size {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "Vulkan allocation exceeds DMA-BUF",
      ));
    }
    let bits =
      requirements.memory_type_bits & imported.gpu.memory_bits(fd)?;
    if bits == 0 {
      return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no compatible DMA-BUF memory type",
      ));
    }
    let duplicate = fd.try_clone_to_owned()?;
    let mut external = vk::ImportMemoryFdInfoKHR::default()
      .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
      .fd(duplicate.as_raw_fd());
    let info = vk::MemoryAllocateInfo::default()
      .push_next(&mut external)
      .allocation_size(requirements.size)
      .memory_type_index(bits.trailing_zeros());
    imported.memory = unsafe { d.allocate_memory(&info, None) }
      .map_err(|r| {
        io::Error::other(format!("Vulkan import memory: {r:?}"))
      })?;
    // Vulkan takes ownership only after successful allocation.
    let _ = duplicate.into_raw_fd();
    unsafe {
      d.bind_buffer_memory(imported.buffer, imported.memory, 0)
    }
    .map_err(|r| {
      io::Error::other(format!("Vulkan bind import: {r:?}"))
    })?;
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
pub(super) struct Pair([Rc<Imported>; 2]);
impl Pair {
  pub(super) fn descriptors(
    &self,
    gpu: &Device,
  ) -> io::Result<[vk::DescriptorBufferInfo; 2]> {
    if self.0.iter().any(|i| !std::ptr::eq(i.gpu.as_ref(), gpu)) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "conversion imports belong to another Vulkan device",
      ));
    }
    Ok(self.0.each_ref().map(|entry| {
      vk::DescriptorBufferInfo::default()
        .buffer(entry.buffer)
        .range(entry.size)
    }))
  }
}
struct Entry {
  import: Rc<Imported>,
  age: u64,
}
pub(super) struct Cache {
  gpu: Arc<Device>,
  entries: Vec<Entry>,
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
    source: dma_buf::image::Plane<'_>,
    destination: dma_buf::image::Plane<'_>,
  ) -> io::Result<Pair> {
    let source_stat = source.metadata()?;
    let destination_stat = destination.metadata()?;
    if source_stat.identity() == destination_stat.identity() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "conversion images alias the same DMA-BUF",
      ));
    }
    let input = self.get(source.fd(), &source_stat, None)?;
    let output =
      self.get(destination.fd(), &destination_stat, Some(input))?;
    Ok(Pair([
      self.entries[input].import.clone(),
      self.entries[output].import.clone(),
    ]))
  }

  fn get(
    &mut self,
    fd: BorrowedFd<'_>,
    stat: &dma_buf::Metadata,
    avoid: Option<usize>,
  ) -> io::Result<usize> {
    self.clock = self.clock.wrapping_add(1);
    let identity = stat.identity();
    if let Some(index) =
      self.entries.iter().position(|e| e.import.identity == identity)
    {
      self.entries[index].age = self.clock;
      return Ok(index);
    }
    let entry = Entry {
      import: Rc::new(Imported::new(self.gpu.clone(), fd, stat)?),
      age: self.clock,
    };
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
}
