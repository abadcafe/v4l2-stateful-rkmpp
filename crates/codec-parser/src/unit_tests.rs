//! module: codec_parser::unit_tests; responsibility: ordered AU
//! termination.
//! operations: test entrypoints; entities: none; tests:
//! test_termination_preserves_payload_extent_and_final_boundary,
//! test_stream_end_without_sequence_end_retains_exact_extent.
use super::unit::{Boundary, Unit};
#[test]
fn test_termination_preserves_payload_extent_and_final_boundary() {
  let mut unit = Unit::annexb(&[0; 100], 1).unwrap();
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
  let mut unit = Unit::annexb(&[0; 100], 1).unwrap();
  unit.stream_end(60).unwrap();
  assert_eq!(unit.range, 0..60);
  assert_eq!(unit.boundary, Some(Boundary::Stream));
}

#[test]
fn test_annexb_rejects_empty_terminal_nal_but_preserves_zero_padding() {
  for bytes in [&[0, 0, 1][..], &[7, 0, 0, 0, 1]] {
    assert!(Unit::annexb(bytes, 1).is_err());
    assert!(Unit::annexb(bytes, 2).is_err());
  }
  assert!(Unit::annexb(&[0, 0, 1, 0x40], 2).is_err());
  let bytes = [0, 0, 1, 7, 128, 0, 0, 0];
  let unit = Unit::annexb(&bytes, 1).unwrap();
  assert_eq!(unit.range, 0..bytes.len());
}
