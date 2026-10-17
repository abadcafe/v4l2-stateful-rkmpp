//! module: mpp::group_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_new_empty_internal_and_external_groups_can_retire
//! test_commit_and_retire_retain_transferred_buffers
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::MetadataExt;

use super::group::Group;
use crate::dma::Buffer;

#[test]
fn test_new_empty_internal_and_external_groups_can_retire() {
  for ten in [false, true] {
    let mut group = Group::new(ten).unwrap();
    assert!(!group.handle().is_null());
    assert!(!group.in_use());
    group.retire().unwrap();
    group.retire().unwrap();
    assert!(!group.in_use());
  }
}

#[test]
fn test_commit_and_retire_retain_transferred_buffers() {
  let buffer = Buffer::allocate(4096).unwrap();
  let fd = buffer.as_fd().as_raw_fd();
  let witness =
    std::fs::File::from(buffer.as_fd().try_clone_to_owned().unwrap());
  let identity = witness.metadata().unwrap();
  let mut group = Group::new(false).unwrap();
  group.commit(&[(3, buffer)], 4).unwrap();
  let retained =
    std::fs::metadata(format!("/proc/self/fd/{fd}")).unwrap();
  assert_eq!(
    (retained.dev(), retained.ino()),
    (identity.dev(), identity.ino())
  );
  group.retire().unwrap();
  let retained =
    std::fs::metadata(format!("/proc/self/fd/{fd}")).unwrap();
  assert_eq!(
    (retained.dev(), retained.ino()),
    (identity.dev(), identity.ino())
  );
  // Exact release accounting belongs to the isolated service resource
  // test.
  drop(group);
}

#[test]
fn test_pressure_allocates_bounded_private_surfaces_without_reserving_them_for_capture()
 {
  let mut group = Group::new(false).unwrap();
  let buffers: Vec<_> =
    (0..18).map(|i| (i, Buffer::allocate(4096).unwrap())).collect();
  group.commit(&buffers, 18).unwrap();
  let mut held = Vec::new();
  for _ in 0..18 {
    held.push(group.reserve_capture(4096).unwrap().unwrap());
  }
  assert!(!group.available());
  assert!(group.ensure_available().unwrap());
  assert!(group.reserve_capture(4096).unwrap().is_none());
  for _ in 18..32 {
    assert!(group.ensure_available().unwrap());
    let private =
      super::reservation::Reservation::acquire(group.handle(), 4096)
        .unwrap()
        .unwrap();
    assert!(private.index().unwrap() >= 32);
    held.push(private);
  }
  assert!(!group.ensure_available().unwrap());
  drop(held.remove(0));
  assert!(
    group.reserve_capture(4096).unwrap().unwrap().index().unwrap() < 18
  );
}

#[test]
fn test_pressure_does_not_allocate_without_registered_capture_geometry()
{
  let mut group = Group::new(false).unwrap();
  assert!(!group.ensure_available().unwrap());
}

#[test]
fn test_partial_capture_registration_reserves_budget_for_unqueued_slots()
 {
  let mut group = Group::new(false).unwrap();
  group.commit(&[(0, Buffer::allocate(4096).unwrap())], 32).unwrap();
  let held = group.reserve_capture(4096).unwrap().unwrap();
  assert!(!group.ensure_available().unwrap());
  group.commit(&[(31, Buffer::allocate(4096).unwrap())], 32).unwrap();
  assert!(group.ensure_available().unwrap());
  assert!(
    group.commit(&[(32, Buffer::allocate(4096).unwrap())], 32).is_err()
  );
  assert!(group.commit(&[], 18).is_err());
  assert!(
    group.commit(&[(0, Buffer::allocate(4096).unwrap())], 32).is_err()
  );
  drop(held);
}

#[test]
fn test_pin_capture_excludes_all_imported_slots_until_individual_requeue()
 {
  let mut group = Group::new(false).unwrap();
  group
    .commit(
      &[
        (0, Buffer::allocate(4096).unwrap()),
        (1, Buffer::allocate(4096).unwrap()),
      ],
      2,
    )
    .unwrap();
  let already_read = group.reserve_capture(4096).unwrap().unwrap();
  let pins = group.pin_capture().unwrap();
  drop(already_read);
  assert!(!group.available());
  let mut pins = pins.into_iter();
  let first = pins.next().unwrap();
  let index = first.index().unwrap();
  drop(first);
  assert_eq!(
    group.reserve_capture(4096).unwrap().unwrap().index().unwrap(),
    index
  );
  drop(pins);
  assert!(group.available());
}
