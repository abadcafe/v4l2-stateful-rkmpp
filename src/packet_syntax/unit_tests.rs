//! module: packet_syntax::unit_tests; responsibility: ordered AU
//! termination.
use super::unit::{Boundary, Unit};
#[test]
fn test_termination_preserves_payload_extent_and_final_boundary() {
  let mut unit = Unit::annexb(100);
  unit.payload().unwrap();
  unit.sequence_end(40).unwrap();
  assert_eq!(unit.range, 0..40);
  assert_eq!(unit.boundary, Some(Boundary::Sequence));
  assert!(unit.payload().is_err());
  assert!(unit.sequence_end(50).is_err());
  unit.stream_end(60).unwrap();
  assert_eq!(unit.range, 0..40);
  assert_eq!(unit.boundary, Some(Boundary::Stream));
  assert!(unit.payload().is_err());
  assert!(unit.sequence_end(70).is_err());
  assert!(unit.stream_end(80).is_err());
}
#[test]
fn test_stream_end_without_sequence_end_retains_exact_extent() {
  let mut unit = Unit::annexb(100);
  unit.stream_end(60).unwrap();
  assert_eq!(unit.range, 0..60);
  assert_eq!(unit.boundary, Some(Boundary::Stream));
}
