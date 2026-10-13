//! module: dma; responsibility: own DMA-BUF allocations and their CPU
//! access. operations: allocate/import, read/write; entities: Buffer,
//! typed CPU mappings. tests: dma_tests, real-GPU quantize_tests;
//! persistent input maps are private to the crate.
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) mod system;
#[cfg(test)]
mod system_tests;

#[derive(Debug)]
struct Allocation {
  fd: OwnedFd,
  size: usize,
  cpu_busy: AtomicBool,
}
#[derive(Clone, Debug)]
pub struct Buffer(Arc<Allocation>);
impl Buffer {
  pub fn allocate(size: usize) -> io::Result<Self> {
    if size == 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "zero DMA allocation",
      ));
    }
    Self::from_owned(system::allocate(size)?, size)
  }

  pub fn from_owned(fd: OwnedFd, size: usize) -> io::Result<Self> {
    let stat = system::metadata(fd.as_fd())?;
    if size == 0
      || size > isize::MAX as usize
      || stat.st_size < size as i64
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "DMA buffer smaller than declared size",
      ));
    }
    Ok(Self(Arc::new(Allocation {
      fd,
      size,
      cpu_busy: AtomicBool::new(false),
    })))
  }

  pub fn size(&self) -> usize {
    self.0.size
  }

  pub fn read(&self) -> io::Result<ReadMapping> {
    ReadMapping::new(self.clone())
  }

  pub fn write(&self) -> io::Result<WriteMapping> {
    WriteMapping::new(self.clone())
  }

  pub(crate) fn begin_read(&self) -> io::Result<CpuAccess> {
    self.begin(false)
  }

  fn begin(&self, write: bool) -> io::Result<CpuAccess> {
    if self
      .0
      .cpu_busy
      .compare_exchange(
        false,
        true,
        Ordering::AcqRel,
        Ordering::Acquire,
      )
      .is_err()
    {
      return Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "DMA buffer already has a CPU access lease",
      ));
    }
    if let Err(error) = system::sync(self.as_fd(), true, write) {
      self.0.cpu_busy.store(false, Ordering::Release);
      return Err(error);
    }
    Ok(CpuAccess::new(self.clone(), write))
  }

  fn end(&self, write: bool) -> io::Result<()> {
    let result = system::sync(self.as_fd(), false, write);
    self.0.cpu_busy.store(false, Ordering::Release);
    result
  }
}
impl AsFd for Buffer {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.fd.as_fd()
  }
}

pub(crate) struct CpuAccess {
  buffer: Buffer,
  write: bool,
  active: bool,
}
impl CpuAccess {
  fn new(buffer: Buffer, write: bool) -> Self {
    Self { buffer, write, active: true }
  }

  pub(crate) fn finish(mut self) -> io::Result<()> {
    self.active = false;
    self.buffer.end(self.write)
  }
}
impl Drop for CpuAccess {
  fn drop(&mut self) {
    if self.active {
      let _ = self.buffer.end(self.write);
    }
  }
}
struct Map {
  buffer: Buffer,
  ptr: NonNull<u8>,
}
impl Map {
  fn new(buffer: Buffer, write: bool) -> io::Result<Self> {
    let protection =
      libc::PROT_READ | if write { libc::PROT_WRITE } else { 0 };
    // SAFETY: owned fd and validated size remain alive until munmap.
    let raw = unsafe {
      libc::mmap(
        std::ptr::null_mut(),
        buffer.size(),
        protection,
        libc::MAP_SHARED,
        buffer.as_fd().as_raw_fd(),
        0,
      )
    };
    if raw == libc::MAP_FAILED {
      return Err(io::Error::last_os_error());
    }
    let Some(ptr) = NonNull::new(raw.cast()) else {
      unsafe {
        libc::munmap(raw, buffer.size());
      }
      return Err(io::Error::other("null input mapping"));
    };
    Ok(Self { buffer, ptr })
  }

  fn bytes(&self) -> &[u8] {
    unsafe {
      std::slice::from_raw_parts(self.ptr.as_ptr(), self.buffer.size())
    }
  }
}
impl Drop for Map {
  fn drop(&mut self) {
    unsafe {
      libc::munmap(self.ptr.as_ptr().cast(), self.buffer.size());
    }
  }
}
pub struct ReadMapping {
  lease: Option<CpuAccess>,
  map: Map,
}
impl ReadMapping {
  fn new(buffer: Buffer) -> io::Result<Self> {
    let lease = buffer.begin(false)?;
    Ok(Self { map: Map::new(buffer, false)?, lease: Some(lease) })
  }

  pub fn finish(mut self) -> io::Result<()> {
    self.lease.take().unwrap().finish()
  }
}
impl Deref for ReadMapping {
  type Target = [u8];

  fn deref(&self) -> &[u8] {
    self.map.bytes()
  }
}
pub struct WriteMapping {
  lease: Option<CpuAccess>,
  map: Map,
}
impl WriteMapping {
  fn new(buffer: Buffer) -> io::Result<Self> {
    let lease = buffer.begin(true)?;
    Ok(Self { map: Map::new(buffer, true)?, lease: Some(lease) })
  }

  pub fn finish(mut self) -> io::Result<()> {
    self.lease.take().unwrap().finish()
  }
}
impl Deref for WriteMapping {
  type Target = [u8];

  fn deref(&self) -> &[u8] {
    self.map.bytes()
  }
}
impl DerefMut for WriteMapping {
  fn deref_mut(&mut self) -> &mut [u8] {
    unsafe {
      std::slice::from_raw_parts_mut(
        self.map.ptr.as_ptr(),
        self.map.buffer.size(),
      )
    }
  }
}
