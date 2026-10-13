//! module: backend::capture_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_default_empty_capture_has_no_available_slots
//! test_recycle_hold_and_requeue_preserve_slot_ownership
//! test_recycle_during_source_change_does_not_queue_old_capture
//! test_recycle_hold_and_image_reject_unknown_slot
use std::os::fd::{AsFd, AsRawFd};

use super::capture::CapturePool;
use crate::dma::Buffer;

#[test]
fn test_default_empty_capture_has_no_available_slots() {
  let pool = CapturePool::default();
  assert_eq!((pool.id(), pool.len(), pool.next()), (0, 0, None));
}

#[test]
fn test_recycle_hold_and_requeue_preserve_slot_ownership() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut pool = CapturePool::new(7, vec![buffer.clone()]);
  assert_eq!((pool.id(), pool.len(), pool.next()), (7, 1, None));
  assert!(pool.hold(0, None).is_err());
  pool.recycle(0, false).unwrap();
  assert_eq!(pool.next(), Some(0));
  pool.hold(0, None).unwrap();
  assert_eq!(pool.next(), None);
  assert!(pool.hold(0, None).is_err());
  pool.recycle(0, false).unwrap();
  let image = pool.image(0, 64, 32).unwrap();
  assert_eq!(
    image.plane().fd().as_raw_fd(),
    buffer.as_fd().as_raw_fd()
  );
  assert_eq!((image.plane().stride(), image.plane().rows()), (64, 32));
}

#[test]
fn test_recycle_during_source_change_does_not_queue_old_capture() {
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  pool.recycle(0, false).unwrap();
  pool.hold(0, None).unwrap();
  pool.recycle(0, true).unwrap();
  assert_eq!(pool.next(), None);
  pool.recycle(0, false).unwrap();
  assert_eq!(pool.next(), Some(0));
}

#[test]
fn test_recycle_hold_and_image_reject_unknown_slot() {
  let mut pool = CapturePool::default();
  assert!(pool.recycle(0, false).is_err());
  assert!(pool.hold(0, None).is_err());
  assert!(pool.image(0, 64, 32).is_err());
}
