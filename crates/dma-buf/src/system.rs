//! module: dma_buf::system; responsibility: perform Linux DMA
//! allocation and synchronization. operations: allocate, metadata, CPU
//! sync, read/write fence export; entities: none. tests: system_tests,
//! dma_tests, quantize_tests.
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
pub(super) fn allocate(size: usize) -> io::Result<OwnedFd> {
  let heap = OpenOptions::new()
    .read(true)
    .write(true)
    .open("/dev/dma_heap/system")?;
  #[repr(C)]
  struct Allocate {
    len: u64,
    fd: u32,
    fd_flags: u32,
    heap_flags: u64,
  }
  let mut allocation = Allocate {
    len: size as u64,
    fd: 0,
    fd_flags: (libc::O_RDWR | libc::O_CLOEXEC) as u32,
    heap_flags: 0,
  };
  const COMMAND: libc::c_ulong =
    (3 << 30) | (24 << 16) | ((b'H' as libc::c_ulong) << 8);
  // SAFETY: Allocate exactly matches dma_heap_allocation_data; ioctl
  // installs one fd.
  if unsafe { libc::ioctl(heap.as_raw_fd(), COMMAND, &mut allocation) }
    < 0
  {
    return Err(io::Error::last_os_error());
  }
  let fd = unsafe { OwnedFd::from_raw_fd(allocation.fd as i32) };
  Ok(fd)
}
#[derive(Clone, Copy, Debug)]
pub struct Metadata {
  pub(crate) st_dev: u64,
  pub(crate) st_ino: u64,
  pub(crate) st_size: i64,
}
pub(crate) fn metadata(fd: BorrowedFd<'_>) -> io::Result<Metadata> {
  let mut stat = std::mem::MaybeUninit::uninit();
  // SAFETY: fstat writes the full stat object on success.
  if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } < 0 {
    return Err(io::Error::last_os_error());
  }
  let stat = unsafe { stat.assume_init() };
  Ok(Metadata {
    st_dev: stat.st_dev,
    st_ino: stat.st_ino,
    st_size: stat.st_size,
  })
}
pub(super) fn sync(
  fd: BorrowedFd<'_>,
  start: bool,
  write: bool,
) -> io::Result<()> {
  let flags: u64 =
    if write { 3 } else { 1 } | if start { 0 } else { 4 };
  const COMMAND: libc::c_ulong =
    (1 << 30) | (8 << 16) | ((b'b' as libc::c_ulong) << 8);
  // SAFETY: flags is dma_buf_sync's complete ABI payload. The CPU
  // ownership layer handles retryable errors before changing state.
  if unsafe { libc::ioctl(fd.as_raw_fd(), COMMAND, &flags) } == 0 {
    Ok(())
  } else {
    Err(io::Error::last_os_error())
  }
}

pub fn export_read(fd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
  export_fence(fd, 1)
}
pub fn export_write(fd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
  export_fence(fd, 2)
}
fn export_fence(fd: BorrowedFd<'_>, flags: u32) -> io::Result<OwnedFd> {
  #[repr(C)]
  struct Export {
    flags: u32,
    fd: i32,
  }
  let mut export = Export { flags, fd: -1 };
  const COMMAND: libc::c_ulong =
    (3 << 30) | (8 << 16) | ((b'b' as libc::c_ulong) << 8) | 2;
  // SAFETY: Export matches dma_buf_export_sync_file; successful ioctl
  // returns one owned fd.
  if unsafe { libc::ioctl(fd.as_raw_fd(), COMMAND, &mut export) } < 0 {
    return Err(io::Error::last_os_error());
  }
  if export.fd < 0 {
    return Err(io::Error::other("invalid exported fence fd"));
  }
  Ok(unsafe { OwnedFd::from_raw_fd(export.fd) })
}

impl Metadata {
  pub fn identity(self) -> (u64, u64) {
    (self.st_dev, self.st_ino)
  }

  pub fn size(self) -> i64 {
    self.st_size
  }
}
