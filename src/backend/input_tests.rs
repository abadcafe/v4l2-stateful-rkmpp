//! module: backend::input_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_prepare_complete_reuse_enforces_one_input_lease
//! test_prepare_and_complete_reject_invalid_index_and_length
//! test_new_rejects_oversized_input_allocation
//! test_drop_releases_active_input_read_lease
use super::input::InputPool;
use crate::dma::Buffer;
use crate::timestamp::Source;

#[test]
fn test_damage_only_marks_the_matching_live_submission() {
  let mut pool =
    InputPool::new(1, vec![Buffer::allocate(4096).unwrap()]).unwrap();
  let first = pool.prepare(0, 0, 4, 7, Source::End).unwrap();
  assert!(pool.damage(&[first]));
  assert!(pool.damage(&[first]));
  assert!(pool.complete(0).unwrap());
  let second = pool.prepare(0, 0, 4, 7, Source::End).unwrap();
  assert_ne!(first, second);
  assert!(!pool.damage(&[first]));
  assert!(!pool.complete(0).unwrap());
  assert!(!pool.damage(&[second]));
  let mut replacement =
    InputPool::new(2, vec![Buffer::allocate(4096).unwrap()]).unwrap();
  let third = replacement.prepare(0, 0, 4, 7, Source::End).unwrap();
  assert_ne!(third, first);
  assert!(!replacement.damage(&[first, second]));
  assert!(!replacement.complete(0).unwrap());
}

#[test]
fn test_prepare_complete_reuse_enforces_one_input_lease() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut pool = InputPool::new(9, vec![buffer.clone()]).unwrap();
  assert_eq!(pool.id(), 9);
  pool.prepare(0, 0, 4096, 17, Source::End).unwrap();
  assert!(pool.prepare(0, 0, 1, 18, Source::End).is_err());
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert!(pool.task(0, 0..1, 1, Source::End).is_ok());
  pool.complete(0).unwrap();
  assert!(pool.complete(0).is_err());
  buffer.write().unwrap().finish().unwrap();
  pool.prepare(0, 0, 1, 18, Source::End).unwrap();
  pool.complete(0).unwrap();
}

#[test]
fn test_prepare_and_complete_reject_invalid_index_and_length() {
  let mut pool =
    InputPool::new(1, vec![Buffer::allocate(4096).unwrap()]).unwrap();
  assert!(pool.prepare(1, 0, 1, 0, Source::End).is_err());
  assert!(pool.complete(1).is_err());
  assert!(pool.task(1, 0..1, 1, Source::End).is_err());
  assert!(pool.task(0, 0..1, 1, Source::End).is_err());
  assert!(pool.prepare(0, 0, 0, 0, Source::End).is_err());
  assert!(pool.prepare(0, 0, 4097, 0, Source::End).is_err());
  pool.prepare(0, 0, 4096, 0, Source::End).unwrap();
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
  pool.prepare(0, 0, 16, 0, Source::End).unwrap();
  drop(pool);
  buffer.write().unwrap().finish().unwrap();
}
