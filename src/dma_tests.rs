//! module: dma_tests; responsibility: define the corresponding public
//! module contract. operations: Rust test entrypoints; entities: none;
//! tests:
//! test_allocate_rejects_zero_size_without_allocating
//! test_from_owned_rejects_file_smaller_than_claimed_size
//! test_write_shared_ownership_and_cpu_access
//! test_read_finish_and_drop_release_exclusive_cpu_access
//! test_from_owned_non_dma_mapping_error_does_not_poison_cpu_access
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};

use crate::dma::Buffer;

#[test]
fn test_allocate_rejects_zero_size_without_allocating() {
  assert!(Buffer::allocate(0).is_err());
}

#[test]
fn test_from_owned_rejects_file_smaller_than_claimed_size() {
  let fd = File::open("/dev/null").unwrap().into();
  assert!(Buffer::from_owned(fd, 4096).is_err());
  let fd = File::open("/dev/null").unwrap().into();
  assert!(Buffer::from_owned(fd, usize::MAX).is_err());
}

#[test]
fn test_write_shared_ownership_and_cpu_access() {
  let buffer = Buffer::allocate(4096).unwrap();
  let fd = buffer.as_fd().as_raw_fd();
  assert_eq!(fd, buffer.clone().as_fd().as_raw_fd());
  let other = buffer.clone();
  {
    let mut bytes = buffer.write().unwrap();
    bytes[0] = 67;
    bytes[4095] = 193;
    assert!(other.read().is_err());
    bytes.finish().unwrap();
  }
  drop(buffer);
  {
    let bytes = other.read().unwrap();
    assert_eq!((bytes[0], bytes[4095]), (67, 193));
  }
  drop(other);
}

#[test]
fn test_read_finish_and_drop_release_exclusive_cpu_access() {
  let buffer = Buffer::allocate(4096).unwrap();
  let access = buffer.read().unwrap();
  assert_eq!(access.len(), 4096);
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  access.finish().unwrap();
  drop(buffer.read().unwrap());
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_from_owned_non_dma_mapping_error_does_not_poison_cpu_access() {
  let raw = unsafe {
    libc::memfd_create(c"dma_cpu_error".as_ptr(), libc::MFD_CLOEXEC)
  };
  assert!(raw >= 0);
  assert_eq!(unsafe { libc::ftruncate(raw, 4096) }, 0);
  use std::os::fd::{FromRawFd, OwnedFd};
  let buffer =
    Buffer::from_owned(unsafe { OwnedFd::from_raw_fd(raw) }, 4096)
      .unwrap();
  for _ in 0..2 {
    assert_eq!(
      buffer.read().err().unwrap().raw_os_error(),
      Some(libc::ENOTTY)
    );
  }
  assert_eq!(buffer.size(), 4096);
}
