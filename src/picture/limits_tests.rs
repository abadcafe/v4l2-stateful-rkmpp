//! module: picture::limits_tests; responsibility: specify buffering
//! bounds. operations: tests; entities: none; tests: capacity, reorder
//! and latency.
use super::limits::Limits;
#[test]
fn test_new_validates_capacity_reorder_and_latency() {
  let limits = Limits::new(4, 2, Some(3)).unwrap();
  assert_eq!(limits.capacity(), 4);
  assert_eq!(limits.reorder(), 2);
  assert_eq!(limits.latency(), Some(3));
  assert!(Limits::new(17, 0, None).is_err());
  assert!(Limits::new(2, 3, None).is_err());
  assert_eq!(Limits::new(0, 0, None).unwrap().capacity(), 1);
}
