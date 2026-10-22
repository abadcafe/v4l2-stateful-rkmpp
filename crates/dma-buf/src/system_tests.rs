//! module: dma_buf::system_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_allocate_returns_cloexec_dma_buf_with_page_aligned_size
//! test_sync_cpu_and_read_write_fences_on_dma_buf
//! test_allocate_rejects_zero_size
//! test_sync_and_fence_export_reject_non_dma_file
use std::os::fd::{AsFd, AsRawFd};

use super::system;

#[test]
fn test_allocate_returns_cloexec_dma_buf_with_page_aligned_size() {
  let fd = system::allocate(4097).unwrap();
  let stat = system::metadata(fd.as_fd()).unwrap();
  assert!(stat.size() >= 4097);
  assert_ne!(
    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) }
      & libc::FD_CLOEXEC,
    0
  );
}

#[test]
fn test_sync_cpu_and_read_write_fences_on_dma_buf() {
  let fd = system::allocate(4096).unwrap();
  for write in [false, true] {
    system::sync(fd.as_fd(), true, write).unwrap();
    system::sync(fd.as_fd(), false, write).unwrap();
  }
  for fd in [
    system::export_read(fd.as_fd()).unwrap(),
    system::export_write(fd.as_fd()).unwrap(),
  ] {
    let mut poll = libc::pollfd {
      fd: fd.as_raw_fd(),
      events: libc::POLLIN,
      revents: 0,
    };
    // SAFETY: a live owned descriptor and one writable poll record.
    assert_eq!(unsafe { libc::poll(&mut poll, 1, 0) }, 1);
    assert_eq!(poll.revents, libc::POLLIN);
  }
}

#[test]
fn test_allocate_rejects_zero_size() {
  assert!(system::allocate(0).is_err());
}

#[test]
fn test_sync_and_fence_export_reject_non_dma_file() {
  let file = std::fs::File::open("/dev/null").unwrap();
  assert!(system::sync(file.as_fd(), true, false).is_err());
  assert!(system::export_read(file.as_fd()).is_err());
  assert!(system::export_write(file.as_fd()).is_err());
}
