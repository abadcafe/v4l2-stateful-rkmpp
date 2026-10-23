//! module: h264_tests; responsibility: define complete H.264 AU
//! admission. operations: test entrypoints; entities: none; tests:
//! metadata, picture boundaries, reference marking consistency,
//! malformed input and recovery.
use super::h264::Validator;

#[test]
fn test_validate_rejects_reference_flags_inconsistent_with_nal_kind() {
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-two-slices.h264"
  ));
  for positions in [&[4][..], &[33], &[42], &[740, 3146]] {
    let mut bad = good.to_vec();
    for &position in positions {
      // Parameter sets and IDR slices must be references; SEI must not.
      bad[position] ^= if position == 42 { 0x20 } else { 0x60 };
    }
    let mut parser = Validator::new().unwrap();
    assert_eq!(
      parser.validate(&bad, 1).err().unwrap().kind(),
      std::io::ErrorKind::InvalidData,
      "NAL positions {positions:?}"
    );
    assert!(parser.configuration().is_empty());
    parser.validate(good, 1).unwrap();
  }
}

#[test]
fn test_validate_rejects_undefined_slice_type_without_committing_configuration()
 {
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-two-slices.h264"
  ));
  let mut bad = good.to_vec();
  // first_mb_in_slice=0, slice_type=7 (all I) -> 12 (undefined).
  // Both type codes occupy seven bits; the remaining header is
  // unchanged.
  assert_eq!(bad[741], 0x88);
  bad[741] = 0x8d;
  let mut parser = Validator::new().unwrap();
  assert_eq!(
    parser.validate(&bad, 1).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  assert!(parser.configuration().is_empty());
  parser.validate(good, 1).unwrap();
}

#[test]
fn test_validate_rejects_slice_address_outside_picture_and_recovers() {
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-two-slices.h264"
  ));
  // The second slice begins with ue(32). ue(64) has two more bits;
  // it names the first macroblock beyond this 8x8-macroblock picture.
  let start = 3147;
  let payload: Vec<bool> = good[start..]
    .iter()
    .flat_map(|byte| {
      (0..8).rev().map(move |bit| byte & (1 << bit) != 0)
    })
    .collect();
  assert_eq!(
    payload[..11],
    [
      false, false, false, false, false, true, false, false, false,
      false, true
    ]
  );
  let mut bits = vec![false; 6];
  bits.extend([true, false, false, false, false, false, true]);
  bits.extend_from_slice(&payload[11..]);
  let mut bad = good[..start].to_vec();
  bad.extend(bits.chunks(8).map(|chunk| {
    chunk.iter().enumerate().fold(0u8, |byte, (index, &bit)| {
      byte | (u8::from(bit) << (7 - index))
    })
  }));
  let mut parser = Validator::new().unwrap();
  parser.validate(good, 1).unwrap();
  assert_eq!(
    parser.validate(&bad, 1).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  parser.validate(good, 1).unwrap();
}
#[test]
fn test_validate_complete_access_unit_and_metadata() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ));
  let mut v = Validator::new().unwrap();
  assert!(v.validate(&bytes[..4454], 1).is_ok());
  assert!(v.validate(&bytes[4454..6728], 1).is_ok());
  assert!(v.validate(&[0, 0, 1, 11], 1).is_ok());
}
#[test]
fn test_validate_rejects_multiple_pictures_and_truncated_headers() {
  let bytes = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ));
  assert!(
    Validator::new().unwrap().validate(&bytes[..6728], 1).is_err()
  );
  for data in [&[][..], &[0, 0, 1], &[0, 0, 1, 0x65, 0xb8], &[5, 6, 7]]
  {
    assert!(Validator::new().unwrap().validate(data, 1).is_err());
  }
}

#[test]
fn test_validate_rejects_conflicting_idr_marking_and_recovers() {
  // 128x128 x264 IDR with slices=2:threads=1. The second NAL header
  // starts at 3146; its no-prior/long-term flags are bits 38/39.
  let good = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-two-slices.h264"
  ));
  assert_eq!(good[3146], 0x65);
  for mask in [2, 1] {
    let mut bad = good.to_vec();
    bad[3150] ^= mask;
    let mut parser = Validator::new().unwrap();
    assert!(parser.validate(good, 1).is_ok());
    assert!(parser.validate(&bad, 1).is_err());
    assert!(parser.validate(good, 1).is_ok());
  }
}

#[test]
fn test_validate_multi_slice_mmco_operands_must_agree() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-two-slices-mmco.h264"
  ));
  // ffprobe access-unit offsets; x264 slices=2:ref=2:bframes=2.
  let boundaries =
    [0, 3733, 6100, 7264, 8261, 10155, 11415, 12411, 13619];
  assert_eq!(stream.len(), boundaries[8]);
  let mut parser = Validator::new().unwrap();
  for range in boundaries.windows(2) {
    parser.validate(&stream[range[0]..range[1]], 1).unwrap();
  }
  let mut bad = stream[10155..11415].to_vec();
  // Second slice MMCO1 difference_of_pic_nums_minus1: 3 -> 4,
  // equal-length Exp-Golomb codes 00100 -> 00101, bit 47 of the NAL.
  bad[10633 - 10155] ^= 1;
  assert!(parser.validate(&bad, 1).is_err());
  parser.validate(&stream[..3733], 1).unwrap();
}
