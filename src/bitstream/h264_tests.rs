//! module: h264_tests; responsibility: H264 syntax public eligibility.
use super::codec::Syntax;
use super::h264::H264;
#[test]
fn test_inspect_rejects_slice_without_configuration() {
  assert!(H264::default().inspect(&[0, 0, 1, 0x65, 0xb8]).is_err());
}
#[test]
fn test_configuration_restarts_with_exact_retained_parameters() {
  let mut syntax = H264::default();
  let picture = syntax
    .inspect(include_bytes!(
      "../../tests/artifacts/h264-discard-old.h264"
    ))
    .unwrap()
    .unwrap();
  assert!(picture.key && picture.display);
  assert!(!syntax.configuration().is_empty());
  syntax.boundary();
  assert!(syntax.inspect(&[0, 0, 1, 6, 128]).unwrap().is_none());
}
