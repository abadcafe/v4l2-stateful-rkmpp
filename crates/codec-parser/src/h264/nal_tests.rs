//! module: nal_tests; responsibility: specify H.264 NAL header
//! admission. operations: tests; entities: none; tests:
//! reference/nonreference kinds and forbidden-zero-bit rejection before
//! interpreting any payload.
#![forbid(unsafe_code)]
use super::nal::Header;

#[test]
fn test_try_from_admits_reference_flags_required_by_each_nal_kind() {
  for kind in [5, 7, 8, 13, 15] {
    assert!(Header::try_from(kind).is_err());
    for flags in [0x20, 0x40, 0x60] {
      assert_eq!(Header::try_from(kind | flags).unwrap().kind(), kind);
    }
  }
  for kind in [6, 9, 10, 11, 12] {
    assert_eq!(Header::try_from(kind).unwrap().kind(), kind);
    for flags in [0x20, 0x40, 0x60] {
      assert!(Header::try_from(kind | flags).is_err());
    }
  }
  for header in [0x01, 0x21, 0x41, 0x61] {
    assert_eq!(Header::try_from(header).unwrap().kind(), 1);
  }
}

#[test]
fn test_try_from_forbidden_zero_bit_rejects_every_nal_kind() {
  for header in 128..=u8::MAX {
    assert!(Header::try_from(header).is_err());
  }
}
