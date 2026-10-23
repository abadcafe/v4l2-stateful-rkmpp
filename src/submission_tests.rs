//! module: submission_tests; responsibility: bounded compressed payload
//! ownership, exact assembly and transactional failure.
//! operations: test entrypoints; entities: none; tests:
//! test_compose_without_prefix_borrows_the_exact_input_range,
//! test_compose_preserves_configuration_metadata_and_picture_once,
//! test_append_and_compose_enforce_bound_before_mutating_prefix,
//! test_compose_large_picture_remains_borrowed_with_small_prefix,
//! test_compose_invalid_range_preserves_prefix_until_clear.
use rkmpp::{MAX_PACKET_SIZE, Payload};

use crate::submission::Prefix;
#[test]
fn test_compose_without_prefix_borrows_the_exact_input_range() {
  let mut prefix = Prefix::default();
  assert!(
    matches!(prefix.compose(&[], &[1,2,3], 1..3).unwrap(), Payload::Input(r) if r == (1..3))
  );
}
#[test]
fn test_compose_preserves_configuration_metadata_and_picture_once() {
  let mut prefix = Prefix::default();
  prefix.append(&[3, 4]).unwrap();
  let Payload::Prefixed { prefix: bytes, range } =
    prefix.compose(&[1, 2], &[9, 5, 6], 1..3).unwrap()
  else {
    panic!("prefix retains only metadata and the input range");
  };
  assert_eq!(bytes, [1, 2, 3, 4]);
  assert_eq!(range, 1..3);
  assert!(matches!(
    prefix.compose(&[], &[7], 0..1).unwrap(),
    Payload::Input(_)
  ));
}
#[test]
fn test_append_and_compose_enforce_bound_before_mutating_prefix() {
  let mut prefix = Prefix::default();
  prefix.append(&vec![1; MAX_PACKET_SIZE - 1]).unwrap();
  assert!(prefix.append(&[2, 3]).is_err());
  assert!(prefix.compose(&[2], &[3], 0..1).is_err());
  let Payload::Prefixed { prefix: bytes, range } =
    prefix.compose(&[], &[4], 0..1).unwrap()
  else {
    panic!("retained prefix");
  };
  assert_eq!(bytes.len(), MAX_PACKET_SIZE - 1);
  assert_eq!(range, 0..1);
}
#[test]
fn test_compose_large_picture_remains_borrowed_with_small_prefix() {
  let picture = vec![9; MAX_PACKET_SIZE - 4];
  let mut metadata = Prefix::default();
  metadata.append(&[3, 4]).unwrap();
  let Payload::Prefixed { prefix, range } =
    metadata.compose(&[1, 2], &picture, 0..picture.len()).unwrap()
  else {
    panic!("metadata and picture must retain separate ownership");
  };
  assert_eq!(prefix, [1, 2, 3, 4]);
  assert_eq!(range, 0..picture.len());
}
#[test]
fn test_compose_invalid_range_preserves_prefix_until_clear() {
  let mut prefix = Prefix::default();
  prefix.append(&[1]).unwrap();
  for (start, end) in [(0, 0), (1, 0), (0, 2)] {
    let range = std::ops::Range { start, end };
    assert!(prefix.compose(&[], &[2], range).is_err());
  }
  prefix.clear();
  assert!(matches!(
    prefix.compose(&[], &[3], 0..1).unwrap(),
    Payload::Input(_)
  ));
}
