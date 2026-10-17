//! module: bitstream::codec_tests; responsibility: Annex B NAL
//! boundaries.
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
