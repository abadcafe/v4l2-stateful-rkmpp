//! module: bitstream::codec_tests; responsibility: Annex B NAL
//! boundaries.
//! operations: test entrypoints; entities: none; tests:
//! test_nals_handles_mixed_start_codes_and_adjacent_empty_units,
//! test_nals_rejects_unframed_bytes_without_panicking,
//! test_nals_excludes_annexb_framing_zeros_from_parameter_payload.
use super::codec::nals;
#[test]
fn test_nals_handles_mixed_start_codes_and_adjacent_empty_units() {
  let bytes = [0, 0, 0, 1, 7, 42, 0, 0, 1, 8, 0, 0, 1, 0, 0, 1, 5];
  assert_eq!(
    nals(&bytes).collect::<Vec<_>>(),
    vec![&[7, 42][..], &[8], &[], &[5]]
  );
}
#[test]
fn test_nals_rejects_unframed_bytes_without_panicking() {
  for bytes in [&[][..], &[0, 0], &[1, 2, 3]] {
    assert_eq!(nals(bytes).count(), 0);
  }
}

#[test]
fn test_nals_excludes_annexb_framing_zeros_from_parameter_payload() {
  for padding in [0, 1, 2, 64, 4096] {
    let mut bytes = vec![0, 0, 0, 1, 0x67, 0x80];
    bytes.extend(std::iter::repeat_n(0, padding));
    bytes.extend([0, 0, 0, 1, 0x68, 0x80]);
    bytes.extend(std::iter::repeat_n(0, padding));
    assert_eq!(
      nals(&bytes).collect::<Vec<_>>(),
      vec![&[0x67, 0x80][..], &[0x68, 0x80]],
      "Annex B framing must not become retained parameter bytes"
    );
  }
}
