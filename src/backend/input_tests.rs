//! module: backend::input_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_prepare_complete_reuse_enforces_one_input_lease
//! test_prepare_and_complete_reject_invalid_index_and_length
//! test_new_rejects_oversized_input_allocation
//! test_drop_releases_active_input_read_lease
use super::input::InputPool;
use crate::dma::Buffer;

#[test]
fn test_prepare_complete_reuse_enforces_one_input_lease() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut pool = InputPool::new(9, vec![buffer.clone()]).unwrap();
  assert_eq!(pool.id(), 9);
  pool.prepare(0, 4096, 17).unwrap();
  assert!(pool.prepare(0, 1, 18).is_err());
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert!(pool.packet(0).is_ok());
  pool.complete(0).unwrap();
  assert!(pool.complete(0).is_err());
  buffer.write().unwrap().finish().unwrap();
  pool.prepare(0, 1, 18).unwrap();
  pool.complete(0).unwrap();
}

#[test]
fn test_prepare_and_complete_reject_invalid_index_and_length() {
  let mut pool =
    InputPool::new(1, vec![Buffer::allocate(4096).unwrap()]).unwrap();
  assert!(pool.prepare(1, 1, 0).is_err());
  assert!(pool.complete(1).is_err());
  assert!(pool.packet(1).is_err());
  assert!(pool.packet(0).is_ok());
  assert!(pool.prepare(0, 0, 0).is_err());
  assert!(pool.prepare(0, 4097, 0).is_err());
  pool.prepare(0, 4096, 0).unwrap();
  pool.complete(0).unwrap();
}

#[test]
fn test_new_rejects_oversized_input_allocation() {
  assert!(
    InputPool::new(
      1,
      vec![Buffer::allocate(16 * 1024 * 1024 + 1).unwrap()]
    )
    .is_err()
  );
}

#[test]
fn test_drop_releases_active_input_read_lease() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut pool = InputPool::new(1, vec![buffer.clone()]).unwrap();
  pool.prepare(0, 16, 0).unwrap();
  drop(pool);
  buffer.write().unwrap().finish().unwrap();
}
