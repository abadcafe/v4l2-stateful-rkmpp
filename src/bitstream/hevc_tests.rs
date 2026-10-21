//! module: hevc_tests; responsibility: HEVC syntax output eligibility.
use super::codec::Syntax;
use super::hevc::Hevc;
#[test]
fn test_inspect_first_idr_selects_active_temporal_limits() {
  let mut h = Hevc::default();
  let bytes =
    include_bytes!("../../tests/artifacts/hevc-hidden-picture.hevc");
  let bytes = &bytes[..3770]; // First independently framed AU.
  let unit = codec_parser::Validator::new(u32::from_le_bytes(*b"HEVC"))
    .unwrap()
    .validate(bytes, 32)
    .unwrap()
    .remove(0);
  let p = h.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
  assert!(p.display && p.key);
  assert_eq!(p.geometry.limits.reorder(), 0);
}
