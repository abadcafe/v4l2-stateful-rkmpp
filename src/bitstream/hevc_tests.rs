//! module: hevc_tests; responsibility: HEVC syntax output eligibility.
use super::codec::Syntax;
use super::hevc::Hevc;
#[test]
fn test_inspect_rejects_slice_without_configuration() {
  assert!(Hevc::default().inspect(&[0, 0, 1, 38, 1, 0xa0]).is_err());
}
#[test]
fn test_inspect_first_idr_selects_active_temporal_limits() {
  let mut h = Hevc::default();
  let p = h
    .inspect(include_bytes!(
      "../../tests/artifacts/hevc-hidden-picture.hevc"
    ))
    .unwrap()
    .unwrap();
  assert!(p.display && p.key);
  assert_eq!(p.geometry.limits.reorder(), 0);
  assert!(!h.configuration().is_empty());
}
