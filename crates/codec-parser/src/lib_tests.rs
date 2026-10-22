//! module: codec_parser_tests; responsibility: specify codec
//! admission. operations: tests; entities: none; tests: codec routing
//! and isolation.
use crate::Validator;

#[test]
fn test_new_rejects_unknown_codecs() {
  for codec in [0, u32::from_le_bytes(*b"NONE")] {
    assert!(Validator::new(codec).is_err());
  }
}
#[test]
fn test_validate_reports_damaged_headers_for_every_framed_codec() {
  for (codec, damaged) in [
    (*b"H264", &[0, 0, 1, 0x65, 0xb8][..]),
    (*b"HEVC", &[0, 0, 1, 0x26, 1, 0xe0][..]),
    (*b"VP80", &[0x10, 0, 0, 0x9d, 1, 0x2a, 0, 1, 0, 0][..]),
    (*b"VP90", &[0x82][..]),
    (*b"AV01", &[0x32, 1, 0xff][..]),
  ] {
    let mut validator =
      Validator::new(u32::from_le_bytes(codec)).unwrap();
    for bytes in [&[][..], &[0xff; 16][..], damaged] {
      assert_eq!(
        validator.validate(bytes, 32).err().unwrap().kind(),
        std::io::ErrorKind::InvalidData
      );
    }
  }
}

#[test]
fn test_validate_semantic_level_error_is_damaged_input_and_preserves_configuration()
 {
  let good = &include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ))[..4454];
  let mut damaged = good.to_vec();
  let sps = damaged
    .windows(4)
    .position(|bytes| bytes == [0, 0, 1, 0x67])
    .unwrap();
  assert_eq!(damaged[sps + 6], 11);
  damaged[sps + 6] = 10;
  for established in [false, true] {
    let mut validator =
      Validator::new(u32::from_le_bytes(*b"H264")).unwrap();
    if established {
      validator.validate(good, 1).unwrap();
    }
    let configuration = validator.configuration();
    assert_eq!(
      validator.validate(&damaged, 1).err().unwrap().kind(),
      std::io::ErrorKind::InvalidData
    );
    assert_eq!(validator.configuration(), configuration);
    assert_eq!(validator.validate(good, 1).unwrap().len(), 1);
  }
}

#[test]
fn test_validate_budget_rejection_cannot_commit_parameter_sets() {
  for (codec, packet) in [
    (
      *b"H264",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/h264-yuv420p.h264"
      ))[..4454],
    ),
    (
      *b"HEVC",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/hevc-yuv420p.hevc"
      ))[..5496],
    ),
  ] {
    let mut validator =
      Validator::new(u32::from_le_bytes(codec)).unwrap();
    assert!(validator.validate(packet, 0).is_err());
    assert!(validator.configuration().is_empty());
    assert_eq!(validator.validate(packet, 1).unwrap().len(), 1);
  }
}

#[test]
fn test_validate_budget_rejection_cannot_initialize_prediction() {
  for (codec, ivf) in [
    (
      *b"VP80",
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/vp8-yuv420p.ivf"
      ))
      .as_slice(),
    ),
    (
      *b"VP90",
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/vp9-yuv420p.ivf"
      ))
      .as_slice(),
    ),
  ] {
    let size =
      u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
    let key = &ivf[44..44 + size];
    let next = 44 + size;
    let size =
      u32::from_le_bytes(ivf[next..next + 4].try_into().unwrap())
        as usize;
    let predicted = &ivf[next + 12..next + 12 + size];
    for established in [false, true] {
      let mut validator =
        Validator::new(u32::from_le_bytes(codec)).unwrap();
      if established {
        validator.validate(key, 1).unwrap();
      }
      assert!(validator.validate(key, 0).is_err());
      assert!(validator.validate(predicted, 1).is_err());
      validator.validate(key, 1).unwrap();
      validator.validate(predicted, 1).unwrap();
    }
  }
}
