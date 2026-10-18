//! module: header_tests; responsibility: owned admission-result
//! lifetime.

#[test]
fn test_validate_annexb_retains_owned_picture_headers() {
  use crate::packet_syntax::header::{Header, Marking};
  let bytes = include_bytes!("../../tests/artifacts/h264-yuv420p.h264");
  let first = {
    let mut parser = crate::packet_syntax::Validator::new(
      u32::from_le_bytes(*b"H264"),
    )
    .unwrap();
    let first = parser.validate(&bytes[..4454]).unwrap().remove(0);
    parser.validate(&bytes[4454..6728]).unwrap();
    first
  };
  assert!(first.prefix > 0 && first.prefix < first.range.end);
  let Some(Header::H264(header)) = first.header else {
    panic!("missing H264 header")
  };
  assert_eq!(header.frame, 0);
  assert_eq!(header.field, None);
  assert!(matches!(header.marking, Marking::Idr { .. }));
  let bytes = include_bytes!("../../tests/artifacts/hevc-yuv420p.hevc");
  let first = {
    let mut parser = crate::packet_syntax::Validator::new(
      u32::from_le_bytes(*b"HEVC"),
    )
    .unwrap();
    let first = parser.validate(&bytes[..5496]).unwrap().remove(0);
    parser.validate(&bytes[5496..7063]).unwrap();
    first
  };
  assert!(first.prefix > 0 && first.prefix < first.range.end);
  let Some(Header::Hevc(header)) = first.header else {
    panic!("missing HEVC header")
  };
  assert!(matches!(header.kind, 19 | 20));
  assert!(header.display);
  assert!(header.short.is_empty() && header.long.is_empty());
}
