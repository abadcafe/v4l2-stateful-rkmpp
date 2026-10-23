//! module: dma_buf::memory; responsibility: own DMA-BUF allocations and
//! their CPU access. operations: allocate/import, read/write; entities:
//! Buffer, typed CPU mappings. tests: memory_tests, real-GPU
//! quantize_tests; persistent reads retain their CPU access leases.
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::sync::Arc;

use crate::{access, storage, system};

pub mod persistent;
#[cfg(test)]
mod persistent_tests;

#[derive(Debug)]
struct Allocation {
  fd: OwnedFd,
  size: usize,
  metadata: system::Metadata,
  cpu_busy: Arc<access::State>,
}
#[derive(Clone, Debug)]
pub struct Buffer(Arc<Allocation>);
impl Buffer {
  pub fn allocate(size: usize) -> io::Result<Self> {
    if size == 0 || size > isize::MAX as usize {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "DMA allocation outside Rust slice domain",
      ));
    }
    Self::from_owned(system::allocate(size)?, size)
  }

  pub fn from_owned(fd: OwnedFd, size: usize) -> io::Result<Self> {
    let stat = system::metadata(fd.as_fd())?;
    if size == 0
      || size > isize::MAX as usize
      || stat.size() < size as i64
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "DMA buffer smaller than declared size",
      ));
    }
    let cpu_busy = storage::state(stat.identity())?;
    Ok(Self(Arc::new(Allocation {
      fd,
      size,
      metadata: stat,
      cpu_busy,
    })))
  }

  pub fn same_storage(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0.cpu_busy, &other.0.cpu_busy)
  }

  pub fn metadata(&self) -> system::Metadata {
    self.0.metadata
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

  fn begin(&self, write: bool) -> io::Result<CpuAccess> {
    access::Lease::new(self.clone(), write)
  }
}
impl access::Owner for Buffer {
  fn state(&self) -> &access::State {
    &self.0.cpu_busy
  }

  fn synchronize(&self, start: bool, write: bool) -> io::Result<()> {
    system::sync(self.as_fd(), start, write)
  }
}

impl AsFd for Buffer {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.fd.as_fd()
  }
}

type CpuAccess = access::Lease<Buffer>;
struct Map {
  buffer: Buffer,
  mapping: memmap2::Mmap,
}
impl Map {
  fn new(buffer: Buffer) -> io::Result<Self> {
    // SAFETY: validated DMA storage has a fixed allocation size. Map
    // retains its owner, and bytes are exposed only under a CPU lease.
    let mapping = unsafe {
      memmap2::MmapOptions::new()
        .len(buffer.size())
        .map(buffer.as_fd().as_raw_fd())
    }?;
    Ok(Self { buffer, mapping })
  }

  fn bytes(&self) -> &[u8] {
    &self.mapping
  }
}
pub struct ReadMapping {
  lease: CpuAccess,
  map: Map,
}
impl ReadMapping {
  fn new(buffer: Buffer) -> io::Result<Self> {
    let lease = buffer.begin(false)?;
    Ok(Self { map: Map::new(buffer)?, lease })
  }

  pub fn finish(self) -> io::Result<()> {
    self.lease.finish()
  }
}
impl Deref for ReadMapping {
  type Target = [u8];

  fn deref(&self) -> &[u8] {
    self.map.bytes()
  }
}
pub struct WriteMapping {
  lease: CpuAccess,
  mapping: memmap2::MmapMut,
}
impl WriteMapping {
  fn new(buffer: Buffer) -> io::Result<Self> {
    let lease = buffer.begin(true)?;
    // SAFETY: the exclusive CPU lease retains the fixed-size DMA
    // allocation and excludes local CPU/device aliases for every slice
    // borrow. External producers transfer ownership before CPU START.
    let mapping = unsafe {
      memmap2::MmapOptions::new()
        .len(buffer.size())
        .map_mut(buffer.as_fd().as_raw_fd())
    };
    Ok(Self { mapping: mapping?, lease })
  }

  pub fn finish(self) -> io::Result<()> {
    self.lease.finish()
  }
}
impl Deref for WriteMapping {
  type Target = [u8];

  fn deref(&self) -> &[u8] {
    &self.mapping
  }
}
impl DerefMut for WriteMapping {
  fn deref_mut(&mut self) -> &mut [u8] {
    &mut self.mapping
  }
}
