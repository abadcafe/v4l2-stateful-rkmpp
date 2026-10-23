//! module: hevc_tests; responsibility: define complete HEVC AU
//! admission. operations: test entrypoints; entities: none; tests:
//! temporal headers, picture boundaries, malformed input and
//! parameter-state recovery.
use super::hevc::Validator;

#[test]
fn test_validate_rejects_undefined_independent_slice_type() {
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-two-slices.hevc"
  ));
  let payload: Vec<bool> = good[2388..5716]
    .iter()
    .flat_map(|byte| {
      (0..8).rev().map(move |bit| byte & (1 << bit) != 0)
    })
    .collect();
  // After first/no-prior/PPS flags, replace slice_type=2 by undefined
  // 3.
  assert_eq!(payload[..6], [true, false, true, false, true, true]);
  let mut bits = payload[..3].to_vec();
  bits.extend([false, false, true, false, false]);
  bits.extend_from_slice(&payload[6..]);
  let mut bad = good[..2388].to_vec();
  bad.extend(bits.chunks(8).map(|chunk| {
    chunk.iter().enumerate().fold(0u8, |byte, (index, &bit)| {
      byte | (u8::from(bit) << (7 - index))
    })
  }));
  bad.extend_from_slice(&good[5716..]);
  let mut parser = Validator::new().unwrap();
  assert_eq!(
    parser.validate(&bad, 1).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  assert!(parser.configuration().is_empty());
  parser.validate(good, 1).unwrap();
}

#[test]
fn test_validate_rejects_nonfirst_slice_at_picture_origin_and_recovers()
{
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-two-slices.hevc"
  ));
  // The second segment's address occupies bits 19..23 of its NAL.
  // Keep its complete header and payload, replacing CTB address 8 by 0.
  let mut bad = good.to_vec();
  assert_eq!(bad[5721], 0x30);
  bad[5721] &= !0x10;
  let mut parser = Validator::new().unwrap();
  parser.validate(good, 1).unwrap();
  assert_eq!(
    parser.validate(&bad, 1).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  parser.validate(good, 1).unwrap();
}

#[test]
fn test_validate_temporal_id_zero_constraints_before_configuration_commit()
 {
  let good = &include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ))[..5496];
  for kind in [32u8, 33, 36, 37] {
    for temporal_id in [1, 6] {
      for established in [false, true] {
        let mut bad = good.to_vec();
        if kind >= 36 {
          bad.extend_from_slice(&[0, 0, 1, kind << 1, 1]);
        }
        Validator::new().unwrap().validate(&bad, 1).unwrap();
        let offset = bad
          .windows(4)
          .position(|data| data == [0, 0, 1, kind << 1])
          .unwrap()
          + 3;
        assert_eq!(bad[offset + 1], 1);
        bad[offset + 1] = temporal_id + 1;
        let mut validator = Validator::new().unwrap();
        if established {
          validator.validate(good, 1).unwrap();
        }
        let configuration = validator.configuration();
        // Native header parsing does not enforce this NAL constraint;
        // the project owns admission before native state can change.
        let result = validator.validate(&bad, 1);
        assert!(result.is_err());
        assert_eq!(
          result.err().unwrap().kind(),
          std::io::ErrorKind::InvalidData
        );
        assert_eq!(validator.configuration(), configuration);
        validator.validate(good, 1).unwrap();
      }
    }
  }
}

#[test]
fn test_validate_complete_access_unit_and_metadata() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ));
  let mut v = Validator::new().unwrap();
  assert!(v.validate(&bytes[..5496], 1).is_ok());
  assert!(v.validate(&bytes[5496..7063], 1).is_ok());
  assert!(v.validate(&[0, 0, 1, 0x4a, 1], 1).is_ok());
}
#[test]
fn test_validate_rejects_multiple_pictures_and_truncated_headers() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/hevc-yuv420p.hevc"
  ));
  assert!(
    Validator::new().unwrap().validate(&bytes[..7063], 1).is_err()
  );
  for data in
    [&[][..], &[0, 0, 1], &[0, 0, 1, 0x26, 1, 0xe0], &[5, 6, 7]]
  {
    assert!(Validator::new().unwrap().validate(data, 1).is_err());
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
  assert!(validator.validate(good, 1).is_ok());
  assert!(validator.validate(&bad, 1).is_err());
  assert!(validator.validate(good, 1).is_ok());
}
