//! module: h264_tests; responsibility: H264 syntax public eligibility.
use super::codec::Syntax;
use super::h264::H264;
#[test]
fn test_inspect_rejects_slice_without_configuration() {
  use crate::packet_syntax::header::{
    H264 as Header264, Header, Marking,
  };
  let header = Header::H264(Header264 {
    pps: 0,
    frame: 0,
    field: None,
    lsb: 0,
    bottom_delta: 0,
    delta: [0, 0],
    marking: Marking::Idr { discard: false, long: false },
  });
  assert!(H264::default().inspect(&[], Some(&header)).is_err());
}
#[test]
fn test_configuration_restarts_with_exact_retained_parameters() {
  let mut syntax = H264::default();
  let bytes =
    include_bytes!("../../tests/artifacts/h264-discard-old.h264");
  let bytes = &bytes[..2469]; // First independently framed AU.
  let unit =
    crate::packet_syntax::Validator::new(u32::from_le_bytes(*b"H264"))
      .unwrap()
      .validate(bytes)
      .unwrap()
      .remove(0);
  let picture =
    syntax.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
  assert!(picture.key && picture.display);
  assert!(!syntax.configuration().is_empty());
  syntax.boundary();
  assert!(syntax.inspect(&[0, 0, 1, 6, 128], None).unwrap().is_none());
}

#[test]
fn test_inspect_metadata_recovery_point_applies_once_without_replaying_bytes()
 {
  use crate::packet_syntax::header::{
    H264 as Header264, Header, Marking,
  };
  let bytes =
    include_bytes!("../../tests/artifacts/h264-discard-old.h264");
  let mut validator =
    crate::packet_syntax::Validator::new(u32::from_le_bytes(*b"H264"))
      .unwrap();
  let bytes = &bytes[..2469]; // First independently framed AU.
  let unit = validator.validate(bytes).unwrap().remove(0);
  let mut syntax = H264::default();
  syntax.inspect(bytes, unit.header.as_ref()).unwrap();
  syntax.boundary();
  assert!(
    syntax
      .inspect(&[0, 0, 1, 6, 6, 1, 0xc0, 0x80], None)
      .unwrap()
      .is_none()
  );
  let header = Header::H264(Header264 {
    pps: 0,
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
  assert!(syntax.inspect(&[], Some(&header)).unwrap().is_none());
}
