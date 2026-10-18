//! module: hevc_tests; responsibility: HEVC syntax output eligibility.
use super::codec::Syntax;
use super::hevc::Hevc;
#[test]
fn test_inspect_rejects_slice_without_configuration() {
  use crate::packet_syntax::header::{Header, Hevc as Header265};
  let header = Header::Hevc(Header265 {
    pps: 0,
    kind: 19,
    temporal_id: 0,
    no_prior: false,
    display: true,
    lsb: 0,
    short: vec![],
    long: vec![],
  });
  assert!(Hevc::default().inspect(&[], Some(&header)).is_err());
}
#[test]
fn test_inspect_first_idr_selects_active_temporal_limits() {
  let mut h = Hevc::default();
  let bytes =
    include_bytes!("../../tests/artifacts/hevc-hidden-picture.hevc");
  let bytes = &bytes[..3770]; // First independently framed AU.
  let unit =
    crate::packet_syntax::Validator::new(u32::from_le_bytes(*b"HEVC"))
      .unwrap()
      .validate(bytes)
      .unwrap()
      .remove(0);
  let p = h.inspect(bytes, unit.header.as_ref()).unwrap().unwrap();
  assert!(p.display && p.key);
  assert_eq!(p.geometry.limits.reorder(), 0);
  assert!(!h.configuration().is_empty());
}
