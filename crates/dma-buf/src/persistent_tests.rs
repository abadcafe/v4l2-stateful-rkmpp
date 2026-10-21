//! module: dma_buf::persistent_tests; responsibility: define checked
//! persistent input reads through the public mapping interface.
//! operations: test entrypoints; entities: none; tests: read ownership
//! and boundaries.
use std::rc::Rc;

use super::persistent::InputMap;
use crate::Buffer;

#[test]
fn test_read_retains_mapping_and_lease_after_input_owner_drops() {
  let buffer = Buffer::allocate(4096).unwrap();
  buffer.write().unwrap()[16..20].copy_from_slice(b"test");
  let input = Rc::new(InputMap::new(buffer.clone()).unwrap());
  let access = input.read(16, 4).unwrap();
  drop(input);
  assert_eq!(access.bytes(), b"test");
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  access.finish().unwrap();
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_read_rejects_overlapping_cpu_access_and_releases_on_drop() {
  let buffer = Buffer::allocate(4096).unwrap();
  let input = Rc::new(InputMap::new(buffer.clone()).unwrap());
  let write = buffer.write().unwrap();
  assert!(input.read(0, 1).is_err());
  drop(write);
  let access = input.read(0, 1).unwrap();
  assert!(input.read(1, 1).is_err());
  drop(access);
  input.read(0, 4096).unwrap().finish().unwrap();
}

#[test]
fn test_read_rejects_empty_out_of_bounds_and_overflow_without_acquiring_lease()
 {
  let buffer = Buffer::allocate(4096).unwrap();
  let input = Rc::new(InputMap::new(buffer.clone()).unwrap());
  for (offset, length) in
    [(0, 0), (4096, 1), (0, 4097), (usize::MAX, 1), (1, usize::MAX)]
  {
    assert_eq!(
      input.read(offset, length).err().unwrap().kind(),
      std::io::ErrorKind::InvalidInput
    );
    buffer.write().unwrap().finish().unwrap();
  }
  assert_eq!(input.read(4095, 1).unwrap().bytes().len(), 1);
}
