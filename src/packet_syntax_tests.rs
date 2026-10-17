//! module: packet_syntax_tests; responsibility: specify codec
//! admission. operations: tests; entities: none; tests: codec routing
//! and isolation.
use crate::packet_syntax::Validator;

#[test]
fn test_new_rejects_unknown_codecs() {
  for codec in [0, u32::from_le_bytes(*b"NONE")] {
    assert!(Validator::new(codec).is_err());
  }
}
#[test]
fn test_validate_reports_damaged_headers_for_every_framed_codec() {
  for (codec, damaged) in [
    (*b"H264", &[0, 0, 1, 0x65, 0xb8][..]),
    (*b"HEVC", &[0, 0, 1, 0x26, 1, 0xe0][..]),
    (*b"VP80", &[0x10, 0, 0, 0x9d, 1, 0x2a, 0, 1, 0, 0][..]),
    (*b"VP90", &[0x82][..]),
    (*b"AV01", &[0x32, 1, 0xff][..]),
  ] {
    let mut validator =
      Validator::new(u32::from_le_bytes(codec)).unwrap();
    for bytes in [&[][..], &[0xff; 16][..], damaged] {
      assert!(validator.validate(bytes).is_err());
    }
  }
}
