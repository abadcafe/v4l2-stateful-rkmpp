//! module: restriction_tests; responsibility: VUI buffering limits.
use super::bits::Bits;
use super::restriction;
fn vui(reorder: u32, buffering: u32) -> Vec<u8> {
  // aspect, overscan, video, chroma, timing, nal/vcl HRD, pic_struct,
  // restriction, motion_vectors_over_pic_boundaries, four ue(0).
  let mut bits = format!("0000000010{}", "1".repeat(4));
  for value in [reorder, buffering] {
    let code = format!("{:b}", value + 1);
    bits.push_str(&"0".repeat(code.len() - 1));
    bits.push_str(&code);
  }
  bits.push('1');
  while !bits.len().is_multiple_of(8) {
    bits.push('0');
  }
  bits
    .as_bytes()
    .chunks(8)
    .map(|c| c.iter().fold(0, |v, b| v * 2 + (b - b'0')))
    .collect()
}
#[test]
fn test_h264_retains_reorder_and_buffering_independently() {
  let limits =
    restriction::h264(&mut Bits::from_rbsp(&vui(2, 4)), 0..=16)
      .unwrap()
      .unwrap();
  assert_eq!(limits.reorder(), 2);
  assert_eq!(limits.capacity(), 4);
}
#[test]
fn test_h264_rejects_inconsistent_or_truncated_restrictions() {
  assert!(
    restriction::h264(&mut Bits::from_rbsp(&vui(3, 2)), 0..=16)
      .is_err()
  );
  assert!(
    restriction::h264(&mut Bits::from_rbsp(&vui(0, 17)), 0..=16)
      .is_err()
  );
  assert!(
    restriction::h264(&mut Bits::from_rbsp(&[]), 0..=16).is_err()
  );
}
#[test]
fn test_h264_absent_restriction_uses_sequence_inference() {
  assert!(
    restriction::h264(&mut Bits::from_rbsp(&[0, 0]), 0..=16)
      .unwrap()
      .is_none()
  );
}
