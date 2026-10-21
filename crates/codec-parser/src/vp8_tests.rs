//! module: vp8_tests; responsibility: define framed header validation.
//! operations: tests; entities: none; tests: valid reference state,
//! damage, recovery, isolated parsers and truncated envelopes.
use super::vp8::Validator;

#[test]
fn test_validate_preserves_sequence_state_and_recovers_at_keyframe() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp8-yuv420p.ivf"
  ));
  for damaged in [
    &[][..],
    &[0xff; 16][..],
    &[0x10, 0, 0, 0x9d, 1, 0x2a, 0, 1, 0, 0][..],
  ] {
    let mut parser = Validator::new().unwrap();
    for _ in 0..2 {
      assert!(parser.validate(damaged).is_err());
      let mut bytes = &stream[32..];
      while !bytes.is_empty() {
        let size =
          u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        parser.validate(&bytes[12..12 + size]).unwrap();
        bytes = &bytes[12 + size..];
      }
    }
  }
}

#[test]
fn test_validate_rejects_truncated_header_without_cross_session_state()
{
  let mut first = Validator::new().unwrap();
  let mut second = Validator::new().unwrap();
  assert!(
    first.validate(&[0x10, 0, 0, 0x9d, 1, 0x2a, 0, 1, 0, 0]).is_err()
  );
  assert!(
    second.validate(&[0x10, 0, 0, 0x9d, 1, 0x2a, 0, 1, 0, 0]).is_err()
  );
}

#[test]
fn test_validate_rejects_bad_dimensions_sync_version_and_partition_bounds()
 {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp8-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  for (offset, value) in [(3, 0), (0, 0x1e), (7, 0), (8, 0), (2, 0xff)]
  {
    let mut damaged = key.to_vec();
    damaged[offset] = value;
    let mut parser = Validator::new().unwrap();
    assert!(parser.validate(&damaged).is_err(), "byte {offset}");
    parser.validate(key).unwrap();
  }
  for length in 0..10 {
    assert!(
      Validator::new().unwrap().validate(&key[..length]).is_err()
    );
  }
}

#[test]
fn test_validate_requires_keyframe_for_initialization_and_damage_recovery()
 {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp8-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let second = 44 + size;
  let size2 =
    u32::from_le_bytes(stream[second..second + 4].try_into().unwrap())
      as usize;
  let key = &stream[44..44 + size];
  let inter = &stream[second + 12..second + 12 + size2];
  let mut parser = Validator::new().unwrap();
  assert!(parser.validate(inter).is_err());
  parser.validate(key).unwrap();
  parser.validate(inter).unwrap();
  assert!(parser.validate(&[0xff]).is_err());
  assert!(parser.validate(inter).is_err());
  parser.validate(key).unwrap();
  parser.validate(inter).unwrap();
}

#[test]
fn test_validate_header_cannot_read_past_declared_first_partition() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp8-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  // Fixture header is 3870 bits; a shorter advertised partition cannot
  // borrow bytes from subsequent coefficient partitions to complete it.
  for declared in [0u32, 483] {
    let mut packet = key.to_vec();
    let tag = (declared << 5) | u32::from(packet[0] & 31);
    packet[..3].copy_from_slice(&tag.to_le_bytes()[..3]);
    let mut parser = Validator::new().unwrap();
    assert!(parser.validate(&packet).is_err());
    parser.validate(key).unwrap();
  }
}
