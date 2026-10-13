//! module: dma::system; responsibility: perform Linux DMA allocation
//! and synchronization. operations: allocate, metadata, CPU sync,
//! read/write fence wait; entities: none. tests: system_tests,
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
pub(crate) fn metadata(fd: BorrowedFd<'_>) -> io::Result<libc::stat> {
  let mut stat = std::mem::MaybeUninit::uninit();
  // SAFETY: fstat writes the full stat object on success.
  if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } < 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(unsafe { stat.assume_init() })
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
  loop {
    // SAFETY: flags is dma_buf_sync's complete ABI payload.
    if unsafe { libc::ioctl(fd.as_raw_fd(), COMMAND, &flags) } == 0 {
      return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() != io::ErrorKind::Interrupted {
      return Err(error);
    }
  }
}

pub(crate) fn wait_read(fd: BorrowedFd<'_>) -> io::Result<()> {
  wait_fence(fd, 1)
}
pub(crate) fn wait_write(fd: BorrowedFd<'_>) -> io::Result<()> {
  wait_fence(fd, 2)
}
fn wait_fence(fd: BorrowedFd<'_>, flags: u32) -> io::Result<()> {
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
  let sync_file = unsafe { OwnedFd::from_raw_fd(export.fd) };
  let mut poll = libc::pollfd {
    fd: sync_file.as_raw_fd(),
    events: libc::POLLIN,
    revents: 0,
  };
  loop {
    // SAFETY: poll addresses one live pollfd.
    let result = unsafe { libc::poll(&mut poll, 1, 2000) };
    if result > 0 && poll.revents & libc::POLLIN != 0 {
      return Ok(());
    }
    if result == 0 {
      return Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "DMA-BUF fence timed out",
      ));
    }
    let error = io::Error::last_os_error();
    if result > 0 || error.kind() != io::ErrorKind::Interrupted {
      return Err(io::Error::other(format!(
        "DMA-BUF fence wait failed: {error}"
      )));
    }
  }
}
