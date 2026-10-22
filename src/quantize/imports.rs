//! module: quantize::imports; responsibility: cache owned Vulkan
//! DMA-BUF imports. operations: new, pair, reserve, descriptors;
//! entities: Cache, Pair, Reservation, Imported; tests: imports_tests,
//! quantize_tests. Preparing a pair does not mutate the cache; only
//! successfully reserved and prepared pairs commit their recency and
//! retention.
use std::collections::VecDeque;
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
  storage: dma_buf::device::Storage,
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
      storage: dma_buf::device::Storage::new(*stat)?,
    };
    // SAFETY: every Vulkan resource below is owned by imported;
    // descriptor chains borrow local storage only until their call
    // returns.
    let d = imported.gpu.api();
    let mut external = vk::ExternalMemoryBufferCreateInfo::default()
      .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let info = vk::BufferCreateInfo::default()
      .push_next(&mut external)
      .size(imported.storage.metadata().size() as u64)
      .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
      .sharing_mode(vk::SharingMode::EXCLUSIVE);
    imported.buffer =
      unsafe { d.create_buffer(&info, None) }.map_err(|r| {
        io::Error::other(format!("Vulkan create import: {r:?}"))
      })?;
    let requirements =
      unsafe { d.get_buffer_memory_requirements(imported.buffer) };
    if requirements.size > imported.storage.metadata().size() as u64 {
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
pub(super) struct Reservation {
  pair: Pair,
  _access: [dma_buf::device::Access; 2],
}
impl Reservation {
  pub(super) fn descriptors(
    &self,
    gpu: &Device,
  ) -> io::Result<[vk::DescriptorBufferInfo; 2]> {
    self.pair.descriptors(gpu)
  }
}
impl Pair {
  fn reserve(self) -> io::Result<Reservation> {
    let access =
      [self.0[0].storage.reserve()?, self.0[1].storage.reserve()?];
    Ok(Reservation { pair: self, _access: access })
  }

  pub(super) fn descriptors(
    &self,
    gpu: &Device,
  ) -> io::Result<[vk::DescriptorBufferInfo; 2]> {
    self.validate(gpu)?;
    Ok(self.0.each_ref().map(|entry| {
      vk::DescriptorBufferInfo::default()
        .buffer(entry.buffer)
        .range(entry.storage.metadata().size() as u64)
    }))
  }

  fn validate(&self, gpu: &Device) -> io::Result<()> {
    if self.0.iter().any(|i| !std::ptr::eq(i.gpu.as_ref(), gpu)) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "conversion imports belong to another Vulkan device",
      ));
    }
    Ok(())
  }
}
pub(super) struct Cache {
  gpu: Arc<Device>,
  entries: VecDeque<Rc<Imported>>,
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
      entries: VecDeque::with_capacity(capacity.get()),
      capacity: capacity.get(),
    })
  }

  pub(super) fn pair(
    &self,
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
    // Retain both owners without changing the cache. A caller may
    // abandon this candidate or fail its access reservation.
    let input = self.get(source.fd(), &source_stat)?;
    let output = self.get(destination.fd(), &destination_stat)?;
    Ok(Pair([input, output]))
  }

  /// Prepare the caller's dependencies while both access leases are
  /// held. Failure drops those leases without committing the cache.
  pub(super) fn reserve<T>(
    &mut self,
    pair: Pair,
    prepare: impl FnOnce() -> io::Result<T>,
  ) -> io::Result<(Reservation, T)> {
    // Validate origin before reserving or mutating either cache.
    pair.validate(&self.gpu)?;
    let reservation = pair.reserve()?;
    let prepared = prepare()?;
    for imported in &reservation.pair.0 {
      self.remember(imported);
    }
    Ok((reservation, prepared))
  }

  fn get(
    &self,
    fd: BorrowedFd<'_>,
    stat: &dma_buf::Metadata,
  ) -> io::Result<Rc<Imported>> {
    let identity = stat.identity();
    if let Some(import) = self
      .entries
      .iter()
      .find(|import| import.storage.metadata().identity() == identity)
    {
      return Ok(import.clone());
    }
    Imported::new(self.gpu.clone(), fd, stat).map(Rc::new)
  }

  fn remember(&mut self, import: &Rc<Imported>) {
    // Two outstanding candidates can import the same storage before
    // either is committed. Keep one entry per allocation identity.
    let cached = self.entries.iter().position(|entry| {
      entry.storage.metadata().identity()
        == import.storage.metadata().identity()
    });
    let retained = if let Some(index) = cached {
      let previous = self.entries.remove(index).unwrap();
      if Rc::ptr_eq(&previous, import) {
        previous
      } else {
        import.clone()
      }
    } else {
      if self.entries.len() == self.capacity {
        self.entries.pop_front();
      }
      import.clone()
    };
    self.entries.push_back(retained);
  }
}
