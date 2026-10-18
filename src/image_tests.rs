//! module: image_tests; responsibility: define the corresponding public
//! module contract. operations: Rust test entrypoints; entities: none;
//! tests:
//! test_new_retains_buffer_and_borrows_layout_after_original_owner_is_dropped
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};

use crate::dma::Buffer;
use crate::image::{Image, Plane};

#[test]
fn test_new_retains_buffer_and_borrows_layout_after_original_owner_is_dropped()
 {
  let raw = unsafe {
    libc::memfd_create(c"image-contract".as_ptr(), libc::MFD_CLOEXEC)
  };
  assert!(raw >= 0);
  let fd = unsafe { OwnedFd::from_raw_fd(raw) };
  assert_eq!(unsafe { libc::ftruncate(raw, 4096) }, 0);
  let buffer = Buffer::from_owned(fd, 4096).unwrap();
  let plane = Plane::new(buffer.as_fd(), 64, 32);
  assert_eq!((plane.stride(), plane.rows()), (64, 32));
  let image = Image::new(buffer.clone(), 128, 16);
  let other = image.clone();
  drop(buffer);
  drop(image);
  assert_eq!(other.plane().fd().as_raw_fd(), raw);
  assert_eq!((other.plane().stride(), other.plane().rows()), (128, 16));
  assert!(unsafe { libc::fcntl(raw, libc::F_GETFD) } >= 0);
}

#[test]
fn test_from_buffer_borrows_registered_storage_and_layout() {
  use std::os::fd::{AsFd, AsRawFd};
  let buffer = crate::dma::Buffer::allocate(4096).unwrap();
  let plane = crate::image::Plane::from_buffer(&buffer, 64, 32);
  assert_eq!(plane.fd().as_raw_fd(), buffer.as_fd().as_raw_fd());
  assert_eq!((plane.stride(), plane.rows()), (64, 32));
  assert_eq!(plane.metadata().unwrap().st_size, 4096);
}
