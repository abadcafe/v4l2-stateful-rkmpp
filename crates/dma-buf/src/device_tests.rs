//! module: dma_buf::device_tests; responsibility: specify local device
//! access ownership. operations: tests; entities: none; tests: access
//! exclusion, alias identity, rollback and release.
use std::io::ErrorKind;
use std::os::fd::AsFd;

use crate::device::Storage;
use crate::memory::Buffer;

#[test]
fn test_reserve_device_access_blocks_cpu_until_release() {
  let buffer = Buffer::allocate(4096).unwrap();
  let storage = Storage::new(buffer.metadata()).unwrap();
  assert_eq!(
    storage.metadata().identity(),
    buffer.metadata().identity()
  );
  let access = storage.reserve().unwrap();
  assert_eq!(
    buffer.read().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  assert_eq!(
    storage.reserve().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  drop(access);
  buffer.write().unwrap().finish().unwrap();
  buffer.read().unwrap().finish().unwrap();
}

#[test]
fn test_reserve_rejects_cpu_access_through_independent_import() {
  let buffer = Buffer::allocate(4096).unwrap();
  let alias = Buffer::from_owned(
    buffer.as_fd().try_clone_to_owned().unwrap(),
    2048,
  )
  .unwrap();
  let cpu = alias.read().unwrap();
  let storage = Storage::new(buffer.metadata()).unwrap();
  assert_eq!(
    storage.reserve().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  cpu.finish().unwrap();
  let device = storage.reserve().unwrap();
  assert_eq!(
    alias.write().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  drop(device);
  alias.write().unwrap().finish().unwrap();
}

#[test]
fn test_reserve_retains_identity_state_across_owner_replacement() {
  let buffer = Buffer::allocate(4096).unwrap();
  let alias = buffer.as_fd().try_clone_to_owned().unwrap();
  let access =
    Storage::new(buffer.metadata()).unwrap().reserve().unwrap();
  drop(buffer);
  let replacement = Buffer::from_owned(alias, 4096).unwrap();
  assert_eq!(
    replacement.read().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  drop(access);
  replacement.read().unwrap().finish().unwrap();
}

#[test]
fn test_new_independent_storage_keeps_independent_ownership() {
  let first = Buffer::allocate(4096).unwrap();
  let second = Buffer::allocate(4096).unwrap();
  let device =
    Storage::new(first.metadata()).unwrap().reserve().unwrap();
  second.write().unwrap().finish().unwrap();
  let other =
    Storage::new(second.metadata()).unwrap().reserve().unwrap();
  drop(device);
  first.write().unwrap().finish().unwrap();
  assert_eq!(
    second.read().err().unwrap().kind(),
    ErrorKind::WouldBlock
  );
  drop(other);
  second.read().unwrap().finish().unwrap();
}
