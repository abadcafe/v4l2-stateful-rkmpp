//! module: header_tests; responsibility: owned admission-result
//! lifetime.
//! operations: test entrypoints; entities: none; tests:
//! test_validate_annexb_retains_owned_picture_headers,
//! test_validate_normalized_sequence_is_shared_and_bad_metadata_is_atomic.

#[test]
fn test_validate_annexb_retains_owned_picture_headers() {
  use crate::header::{Header, Marking};
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ));
  let first = {
    let mut parser =
      crate::Validator::new(u32::from_le_bytes(*b"H264")).unwrap();
    let first =
      parser.validate(&bytes[..4454], 32).unwrap().units.remove(0);
    parser.validate(&bytes[4454..6728], 32).unwrap();
    first
  };
  assert!(first.prefix > 0 && first.prefix < first.range.end);
  let Some(Header::H264(header)) = first.header else {
    panic!("missing H264 header")
  };
  assert_eq!(
    (header.sequence.geometry.width, header.sequence.geometry.height),
    (256, 128)
  );
  assert_eq!(header.frame, 0);
  assert_eq!(header.field, None);
  assert!(matches!(header.marking, Marking::Idr { .. }));
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ));
  let first = {
    let mut parser =
      crate::Validator::new(u32::from_le_bytes(*b"HEVC")).unwrap();
    let first =
      parser.validate(&bytes[..5496], 32).unwrap().units.remove(0);
    parser.validate(&bytes[5496..7063], 32).unwrap();
    first
  };
  assert!(first.prefix > 0 && first.prefix < first.range.end);
  let Some(Header::Hevc(header)) = first.header else {
    panic!("missing HEVC header")
  };
  assert!(matches!(header.kind, 19 | 20));
  assert_eq!(
    (header.sequence.geometry.width, header.sequence.geometry.height),
    (256, 128)
  );
  assert!(header.display);
  assert!(header.short.is_empty() && header.long.is_empty());
}

#[test]
fn test_validate_normalized_sequence_is_shared_and_bad_metadata_is_atomic()
 {
  use crate::Validator;
  use crate::header::Header;
  for (codec, bytes, first_end, second_end) in [
    (
      *b"H264",
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/h264-yuv420p.h264"
      ))
      .as_slice(),
      4454,
      6728,
    ),
    (
      *b"HEVC",
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/hevc-yuv420p.hevc"
      ))
      .as_slice(),
      5496,
      7063,
    ),
  ] {
    let mut parser = Validator::new(u32::from_le_bytes(codec)).unwrap();
    let first =
      parser.validate(&bytes[..first_end], 1).unwrap().units.remove(0);
    let second = parser
      .validate(&bytes[first_end..second_end], 1)
      .unwrap()
      .units
      .remove(0);
    match (first.header.unwrap(), second.header.unwrap()) {
      (Header::H264(first), Header::H264(second)) => {
        assert!(std::rc::Rc::ptr_eq(&first.sequence, &second.sequence))
      }
      (Header::Hevc(first), Header::Hevc(second)) => {
        assert!(std::rc::Rc::ptr_eq(&first.sequence, &second.sequence))
      }
      _ => panic!("wrong normalized codec"),
    }
    let configuration = parser.configuration();
    let mut malformed = configuration.to_vec();
    malformed.extend_from_slice(if codec == *b"H264" {
      &[0, 0, 1, 0x67]
    } else {
      &[0, 0, 1, 0x42, 1]
    });
    assert!(parser.validate(&malformed, 1).is_err());
    assert_eq!(parser.configuration(), configuration);
    assert!(parser.validate(&bytes[..first_end], 1).is_ok());
  }
}
