//! module: pool_tests; responsibility: define the corresponding public
//! module contract. operations: Rust test entrypoints; entities: none;
//! tests:
//! test_new_rejects_missing_identity_empty_and_oversized_registration
//! test_into_parts_transfers_entire_registration_to_consumer
use std::os::fd::{FromRawFd, OwnedFd};

use crate::dma::Buffer;
use crate::pool::{Direction, Pool};

fn buffer() -> Buffer {
  let raw = unsafe {
    libc::memfd_create(c"pool-contract".as_ptr(), libc::MFD_CLOEXEC)
  };
  assert!(raw >= 0);
  let fd = unsafe { OwnedFd::from_raw_fd(raw) };
  assert_eq!(unsafe { libc::ftruncate(raw, 4096) }, 0);
  Buffer::from_owned(fd, 4096).unwrap()
}

#[test]
fn test_new_rejects_missing_identity_empty_and_oversized_registration()
{
  assert!(Pool::new(Direction::Output, 0, vec![buffer()]).is_err());
  assert!(Pool::new(Direction::Capture, 1, vec![]).is_err());
  assert!(
    Pool::new(Direction::Capture, 1, vec![buffer(); 33]).is_err()
  );
}

#[test]
fn test_into_parts_transfers_entire_registration_to_consumer() {
  let pool = Pool::new(
    Direction::Capture,
    7,
    (0..32).map(|_| buffer()).collect(),
  )
  .unwrap();
  let (direction, id, buffers) = pool.into_parts();
  assert_eq!(direction, Direction::Capture);
  assert_eq!(id, 7);
  assert_eq!(buffers.len(), 32);
  assert_eq!(buffers[31].size(), 4096);
}

#[test]
fn test_new_rejects_storage_aliases_for_both_directions() {
  use std::os::fd::AsFd;
  for direction in [Direction::Output, Direction::Capture] {
    let first = buffer();
    let duplicate = Buffer::from_owned(
      first.as_fd().try_clone_to_owned().unwrap(),
      2048,
    )
    .unwrap();
    assert!(
      Pool::new(direction, 1, vec![first.clone(), first.clone()])
        .is_err()
    );
    assert!(Pool::new(direction, 1, vec![first, duplicate]).is_err());
  }
}
