//! module: codec_parser_tests; responsibility: specify codec
//! admission. operations: tests; entities: none; tests: codec routing
//! and isolation.
use crate::Validator;

#[test]
fn test_validate_incomplete_annexb_tail_never_poison_configuration() {
  for (codec, packet, tails) in [
    (
      *b"H264",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/h264-yuv420p.h264"
      ))[..4454],
      &[&[0, 0, 1][..], &[0, 0, 0, 1]][..],
    ),
    (
      *b"HEVC",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/hevc-yuv420p.hevc"
      ))[..5496],
      &[&[0, 0, 1][..], &[0, 0, 0, 1], &[0, 0, 1, 0x40]][..],
    ),
  ] {
    let mut seed = Validator::new(u32::from_le_bytes(codec)).unwrap();
    seed.validate(packet, 1).unwrap();
    let configuration = seed.configuration();
    for established in [false, true] {
      for tail in tails {
        for input in [packet, configuration.as_slice()] {
          let mut parser =
            Validator::new(u32::from_le_bytes(codec)).unwrap();
          if established {
            parser.validate(packet, 1).unwrap();
          }
          let previous = parser.configuration();
          let mut truncated = input.to_vec();
          truncated.extend_from_slice(tail);
          assert!(
            parser.validate(&truncated, 1).is_err(),
            "codec {codec:?}, tail {tail:?}"
          );
          assert_eq!(parser.configuration(), previous);
          parser.reset().unwrap();
          parser.validate(packet, 1).unwrap();
        }
      }
    }
    // Annex B trailing_zero_8bits are framing, not a truncated NAL.
    let mut padded = configuration.to_vec();
    padded.extend([0; 16]);
    seed.validate(&padded, 0).unwrap();
    assert_eq!(seed.configuration(), configuration);
    seed.reset().unwrap();
  }
}

#[test]
fn test_admit_rejected_update_restores_nonempty_preceding_configuration()
 {
  for (codec, previous, replacement) in [
    (
      *b"H264",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/h264-yuv420p.h264"
      ))[..4454],
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/h264-512.h264"
      ))[..7374],
    ),
    (
      *b"HEVC",
      &include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/hevc-yuv420p.hevc"
      ))[..5496],
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/hevc-main10.hevc"
      ))
      .as_slice(),
    ),
  ] {
    let mut parser = Validator::new(u32::from_le_bytes(codec)).unwrap();
    let unit = parser.validate(previous, 1).unwrap().units.remove(0);
    let configuration = parser.configuration();
    let result: std::io::Result<()> =
      parser.admit(replacement, 1, |candidate, _| {
        assert_ne!(candidate.configuration(), configuration);
        Err(std::io::Error::new(
          std::io::ErrorKind::InvalidData,
          "reject changed configuration",
        ))
      });
    assert!(result.is_err());
    assert_eq!(parser.configuration(), configuration);
    assert_eq!(
      parser.validate(&previous[unit.prefix..], 1).unwrap().units.len(),
      1
    );
  }
}

#[test]
fn test_admit_consumer_rejection_restores_preceding_configuration() {
  let ivf = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
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
    (*b"AV01", &ivf[44..44 + size]),
  ] {
    for established in [false, true] {
      let mut validator =
        Validator::new(u32::from_le_bytes(codec)).unwrap();
      if established {
        validator.validate(packet, 1).unwrap();
      }
      let previous = validator.configuration();
      let result: std::io::Result<()> =
        validator.admit(packet, 1, |parser, units| {
          assert_eq!(units.units.len(), 1);
          assert!(!parser.configuration().is_empty());
          Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "semantic rejection",
          ))
        });
      assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
      );
      assert_eq!(validator.configuration(), previous);
      assert_eq!(validator.configuration().as_ptr(), previous.as_ptr());
      assert_eq!(
        validator
          .admit(packet, 1, |_, units| Ok(units.units.len()))
          .unwrap(),
        1
      );
    }
  }
}

#[test]
fn test_reset_preserves_configuration_storage_and_accepts_fresh_keyframes()
 {
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
    validator.validate(packet, 1).unwrap();
    let configuration = validator.configuration();
    for _ in 0..8 {
      validator.reset().unwrap();
      assert_eq!(
        configuration.as_ptr(),
        validator.configuration().as_ptr()
      );
      assert_eq!(configuration, validator.configuration());
      validator.validate(packet, 1).unwrap();
    }
  }
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
    (
      *b"AV01",
      include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/artifacts/av1-yuv420p.ivf"
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
    let mut validator =
      Validator::new(u32::from_le_bytes(codec)).unwrap();
    validator.reset().unwrap();
    validator.validate(key, 32).unwrap();
    let configuration = validator.configuration();
    for _ in 0..8 {
      validator.reset().unwrap();
      assert_eq!(
        configuration.as_ptr(),
        validator.configuration().as_ptr()
      );
      assert!(validator.validate(predicted, 32).is_err());
      validator.validate(key, 32).unwrap();
    }
  }
}

#[test]
fn test_configuration_snapshots_share_storage_through_damage_recovery()
{
  let ivf = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
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
    (*b"AV01", &ivf[44..44 + size]),
  ] {
    let mut validator =
      Validator::new(u32::from_le_bytes(codec)).unwrap();
    validator.validate(packet, 1).unwrap();
    let configuration = validator.configuration();
    assert!(!configuration.is_empty());
    for _ in 0..8 {
      assert_eq!(
        validator.configuration().as_ptr(),
        configuration.as_ptr()
      );
      assert!(validator.validate(&[0xff], 1).is_err());
      assert_eq!(
        validator.configuration().as_ptr(),
        configuration.as_ptr()
      );
      validator.validate(packet, 1).unwrap();
      assert_eq!(
        validator.configuration().as_ptr(),
        configuration.as_ptr()
      );
    }
    drop(validator);
    assert!(!configuration.is_empty());
  }
}

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
fn test_validate_advertised_level_preserves_picture_and_configuration_updates()
 {
  let good = &include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ))[..4454];
  let sps =
    good.windows(4).position(|bytes| bytes == [0, 0, 1, 0x67]).unwrap();
  assert_eq!(good[sps + 6], 11);
  for level in [10, 255] {
    let mut adjusted = good.to_vec();
    adjusted[sps + 6] = level;
    for established in [false, true] {
      let mut validator =
        Validator::new(u32::from_le_bytes(*b"H264")).unwrap();
      if established {
        validator.validate(good, 1).unwrap();
      }
      let configuration = validator.configuration();
      assert_eq!(
        validator.validate(&adjusted, 1).unwrap().units.len(),
        1
      );
      assert_ne!(validator.configuration(), configuration);
      let adjusted_configuration = validator.configuration();
      validator.validate(good, 1).unwrap();
      assert_ne!(validator.configuration(), adjusted_configuration);
    }
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
    assert_eq!(validator.validate(packet, 1).unwrap().units.len(), 1);
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
