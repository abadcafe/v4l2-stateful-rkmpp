//! module: vp9_tests; responsibility: define framed header validation.
//! operations: tests; entities: none; tests: valid reference state,
//! damage, recovery, isolated parsers and truncated envelopes.
use super::vp9::Validator;

#[test]
fn test_validate_preserves_sequence_state_and_recovers_at_keyframe() {
  let stream = include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
  for damaged in [&[][..], &[0xff; 16][..], &[0x82][..]] {
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
  assert!(first.validate(&[0x82]).is_err());
  assert!(second.validate(&[0x82]).is_err());
}

#[test]
fn test_validate_checks_superframe_index_against_every_frame_extent() {
  let stream = include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  let mut packet = [key, key].concat();
  // Two frames, two bytes per length: 110 01 001.
  packet.push(0xc9);
  packet.extend_from_slice(&(size as u16).to_le_bytes());
  packet.extend_from_slice(&(size as u16).to_le_bytes());
  packet.push(0xc9);
  Validator::new().unwrap().validate(&packet).unwrap();
  for (offset, value) in
    [(2 * size, 0xc8), (2 * size + 1, 0xff), (2 * size + 2, 0xff)]
  {
    let mut damaged = packet.clone();
    damaged[offset] = value;
    assert!(Validator::new().unwrap().validate(&damaged).is_err());
  }
  assert!(Validator::new().unwrap().validate(&[0xc9]).is_err());
}

#[test]
fn test_validate_existing_frame_requires_prior_reference_initialization()
 {
  let stream = include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let mut parser = Validator::new().unwrap();
  assert!(parser.validate(&[0x88]).is_err());
  parser.validate(&stream[44..44 + size]).unwrap();
  parser.validate(&[0x88]).unwrap();
  assert!(parser.validate(&[0x82]).is_err());
  assert!(parser.validate(&[0x88]).is_err());
}

#[test]
fn test_validate_compressed_header_must_be_followed_by_tile_data() {
  let stream = include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  // This keyframe has an 18-byte uncompressed header and a 154-byte
  // compressed header. A packet ending there contains no coded tile.
  for length in [18, 171, 172] {
    let mut parser = Validator::new().unwrap();
    assert!(parser.validate(&key[..length]).is_err());
    parser.validate(key).unwrap();
  }
}
