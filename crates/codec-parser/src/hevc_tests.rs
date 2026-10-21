//! Public HEVC complete-access-unit admission contract.
use super::hevc::Validator;
#[test]
fn test_validate_complete_access_unit_and_metadata() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ));
  let mut v = Validator::new().unwrap();
  assert!(v.validate(&bytes[..5496]).is_ok());
  assert!(v.validate(&bytes[5496..7063]).is_ok());
  assert!(v.validate(&[0, 0, 1, 0x4a, 1]).is_ok());
}
#[test]
fn test_validate_rejects_multiple_pictures_and_truncated_headers() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ));
  assert!(Validator::new().unwrap().validate(&bytes[..7063]).is_err());
  for data in
    [&[][..], &[0, 0, 1], &[0, 0, 1, 0x26, 1, 0xe0], &[5, 6, 7]]
  {
    assert!(Validator::new().unwrap().validate(data).is_err());
  }
}

#[test]
fn test_validate_multi_slice_picture_rejects_conflicting_prior_output()
{
  // x265: 256x256 testsrc2, one IDR, slices=2:wpp=1:pools=1.
  // Second IDR segment starts at byte 5719; no_output_of_prior_pics
  // is bit 6 of its first RBSP byte. H.265 7.4.7.1 requires agreement.
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-two-slices.hevc"
  ));
  assert_eq!(&good[5716..5722], &[0, 0, 1, 40, 1, 0x30]);
  let mut bad = good.to_vec();
  bad[5721] ^= 0x40;
  let mut validator = Validator::new().unwrap();
  assert!(validator.validate(good).is_ok());
  assert!(validator.validate(&bad).is_err());
  assert!(validator.validate(good).is_ok());
}
