//! module: h264_tests; responsibility: H264 syntax public eligibility.
//! operations: test entrypoints; entities: none; tests:
//! test_inspect_validated_idr_and_metadata_boundary,
//! test_inspect_metadata_recovery_point_applies_once_without_replaying_bytes.
use super::codec::Syntax;
use super::h264::H264;

#[test]
fn test_boundary_recovery_starts_independent_reference_and_number_history()
 {
  use codec_parser::header::{H264 as Header264, Header, Marking};
  let bytes =
    &include_bytes!("../../tests/artifacts/h264-discard-old.h264")
      [..2469];
  let unit = codec_parser::Validator::new(u32::from_le_bytes(*b"H264"))
    .unwrap()
    .validate(bytes, 1)
    .unwrap()
    .units
    .remove(0);
  let Header::H264(idr) = unit.header.as_ref().unwrap() else {
    panic!("expected H264");
  };
  for frame in [1, 7] {
    let mut syntax = H264::default();
    let previous =
      syntax.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
    syntax.boundary();
    let recovery = Header::H264(Header264 {
      sequence: idr.sequence.clone(),
      frame,
      field: None,
      lsb: 2,
      bottom_delta: 0,
      delta: [0, 0],
      marking: Marking::Sliding,
    });
    let resumed = syntax
      .inspect(&[0, 0, 1, 6, 6, 1, 0xc0, 0x80], Some(&recovery))
      .unwrap()
      .unwrap();
    assert!(resumed.display);
    assert!(resumed.id > previous.id);
    assert!(!resumed.references.contains(&previous.id));
  }
}

#[test]
fn test_inspect_seek_skips_dependent_marking_before_touching_reference_state()
 {
  use codec_parser::header::{
    H264 as Header264, Header, Marking, Operation,
  };
  let bytes =
    &include_bytes!("../../tests/artifacts/h264-discard-old.h264")
      [..2469];
  let unit = codec_parser::Validator::new(u32::from_le_bytes(*b"H264"))
    .unwrap()
    .validate(bytes, 1)
    .unwrap()
    .units
    .remove(0);
  let Header::H264(idr) = unit.header.as_ref().unwrap() else {
    panic!("expected H264");
  };
  let dependent = Header::H264(Header264 {
    sequence: idr.sequence.clone(),
    frame: 1,
    field: None,
    lsb: 2,
    bottom_delta: 0,
    delta: [0, 0],
    marking: Marking::Adaptive(vec![Operation::ForgetShort(0)]),
  });
  let mut syntax = H264::default();
  for _ in 0..2 {
    assert!(syntax.inspect(&[], Some(&dependent)).unwrap().is_none());
    let decoded =
      syntax.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
    assert!(decoded.key);
    syntax.seek();
  }
}
#[test]
fn test_inspect_validated_idr_and_metadata_boundary() {
  let mut syntax = H264::default();
  let bytes =
    include_bytes!("../../tests/artifacts/h264-discard-old.h264");
  let bytes = &bytes[..2469]; // First independently framed AU.
  let unit = codec_parser::Validator::new(u32::from_le_bytes(*b"H264"))
    .unwrap()
    .validate(bytes, 32)
    .unwrap()
    .units
    .remove(0);
  let picture =
    syntax.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
  assert!(picture.key && picture.display);
  syntax.boundary();
  assert!(syntax.inspect(&[0, 0, 1, 6, 128], None).unwrap().is_none());
}

#[test]
fn test_inspect_metadata_recovery_point_applies_once_without_replaying_bytes()
 {
  use codec_parser::header::{H264 as Header264, Header, Marking};
  let bytes =
    include_bytes!("../../tests/artifacts/h264-discard-old.h264");
  let mut validator =
    codec_parser::Validator::new(u32::from_le_bytes(*b"H264")).unwrap();
  let bytes = &bytes[..2469]; // First independently framed AU.
  let unit = validator.validate(bytes, 32).unwrap().units.remove(0);
  let mut syntax = H264::default();
  syntax.inspect(bytes, unit.header.as_ref()).unwrap();
  syntax.boundary();
  assert!(
    syntax
      .inspect(&[0, 0, 1, 6, 6, 1, 0xc0, 0x80], None)
      .unwrap()
      .is_none()
  );
  let mut header = Header::H264(Header264 {
    sequence: match unit.header.as_ref().unwrap() {
      Header::H264(previous) => previous.sequence.clone(),
      _ => panic!("missing H264 header"),
    },
    frame: 1,
    field: None,
    lsb: 2,
    bottom_delta: 0,
    delta: [0, 0],
    marking: Marking::Sliding,
  });
  let picture = syntax.inspect(&[], Some(&header)).unwrap().unwrap();
  assert!(picture.display);
  syntax.boundary();
  let Header::H264(next) = &mut header else {
    panic!("missing H264 header");
  };
  next.frame = 2;
  next.lsb = 4;
  assert!(syntax.inspect(&[], Some(&header)).unwrap().is_none());
}
