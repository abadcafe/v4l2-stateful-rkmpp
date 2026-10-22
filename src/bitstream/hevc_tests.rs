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

#[test]
fn test_inspect_rejects_missing_current_reference_and_resumes_at_idr() {
  let bytes = include_bytes!("../../tests/artifacts/hevc-yuv420p.hevc");
  // ffprobe extents: IDR POC0 [0,5496), reference POC2 [5496,7063),
  // then B POC1 [7063,7618). Losing POC2 leaves a required B reference
  // absent.
  let mut validator =
    codec_parser::Validator::new(u32::from_le_bytes(*b"HEVC")).unwrap();
  let first = validator.validate(&bytes[..5496], 1).unwrap().remove(0);
  let missing =
    validator.validate(&bytes[7063..7618], 1).unwrap().remove(0);
  let mut syntax = Hevc::default();
  syntax
    .inspect(&bytes[..5496], first.header.as_ref())
    .unwrap()
    .unwrap();
  assert!(
    syntax
      .inspect(&bytes[7063..7618], missing.header.as_ref())
      .is_err()
  );
  syntax.seek();
  let resumed = syntax
    .inspect(&bytes[..5496], first.header.as_ref())
    .unwrap()
    .unwrap();
  assert!(resumed.key && resumed.display);
}
