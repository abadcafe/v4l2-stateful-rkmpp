//! module: mpp::status_tests; responsibility: specify SDK error
//! classification. operations: tests; entities: none; tests:
//! backpressure and fatal errors.
use super::status::{frame, reservation};
#[test]
fn test_frame_nonblocking_errors_are_not_empty_frames() {
  frame(0).unwrap();
  for status in [-1, -8, -1005, -1006, -1009] {
    assert!(frame(status).is_err());
  }
}
#[test]
fn test_reservation_only_empty_external_group_is_retryable() {
  assert!(reservation(0).unwrap());
  assert!(!reservation(-1).unwrap());
  for status in [-2, -4, -8, -1006] {
    assert!(reservation(status).is_err());
  }
}
