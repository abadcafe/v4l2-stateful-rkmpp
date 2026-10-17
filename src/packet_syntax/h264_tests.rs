//! Public H.264 complete-access-unit admission contract.
use super::h264::Validator;
#[test]
fn test_validate_complete_access_unit_and_metadata() {
  let bytes = include_bytes!("../../tests/artifacts/h264-yuv420p.h264");
  let mut v = Validator::new().unwrap();
  assert!(v.validate(&bytes[..4454]).is_ok());
  assert!(v.validate(&bytes[4454..6728]).is_ok());
  assert!(v.validate(&[0, 0, 1, 11]).is_ok());
}
#[test]
fn test_validate_rejects_multiple_pictures_and_truncated_headers() {
  let bytes = include_bytes!("../../tests/artifacts/h264-yuv420p.h264");
  assert!(Validator::new().unwrap().validate(&bytes[..6728]).is_err());
  for data in [&[][..], &[0, 0, 1], &[0, 0, 1, 0x65, 0xb8], &[5, 6, 7]]
  {
    assert!(Validator::new().unwrap().validate(data).is_err());
  }
}

#[test]
fn test_validate_rejects_conflicting_idr_marking_and_recovers() {
  // 128x128 x264 IDR with slices=2:threads=1. The second NAL header
  // starts at 3146; its no-prior/long-term flags are bits 38/39.
  let good =
    include_bytes!("../../tests/artifacts/h264-two-slices.h264");
  assert_eq!(good[3146], 0x65);
  for mask in [2, 1] {
    let mut bad = good.to_vec();
    bad[3150] ^= mask;
    let mut parser = Validator::new().unwrap();
    assert!(parser.validate(good).is_ok());
    assert!(parser.validate(&bad).is_err());
    assert!(parser.validate(good).is_ok());
  }
}

#[test]
fn test_validate_multi_slice_mmco_operands_must_agree() {
  let stream =
    include_bytes!("../../tests/artifacts/h264-two-slices-mmco.h264");
  // ffprobe access-unit offsets; x264 slices=2:ref=2:bframes=2.
  let boundaries =
    [0, 3733, 6100, 7264, 8261, 10155, 11415, 12411, 13619];
  assert_eq!(stream.len(), boundaries[8]);
  let mut parser = Validator::new().unwrap();
  for range in boundaries.windows(2) {
    parser.validate(&stream[range[0]..range[1]]).unwrap();
  }
  let mut bad = stream[10155..11415].to_vec();
  // Second slice MMCO1 difference_of_pic_nums_minus1: 3 -> 4,
  // equal-length Exp-Golomb codes 00100 -> 00101, bit 47 of the NAL.
  bad[10633 - 10155] ^= 1;
  assert!(parser.validate(&bad).is_err());
  parser.validate(&stream[..3733]).unwrap();
}
