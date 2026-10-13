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
  group.commit(&[buffer]).unwrap();
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
