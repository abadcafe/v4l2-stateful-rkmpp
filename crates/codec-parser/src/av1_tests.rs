//! module: av1_tests; responsibility: define framed header validation.
//! operations: tests; entities: none; tests: valid reference state,
//! damage, recovery, isolated parsers and truncated envelopes.
use super::av1::Validator;

#[test]
fn test_validate_ignores_obu_reserved_header_bit_without_losing_prediction()
 {
  // AV1 6.2.2 requires decoders to ignore obu_reserved_1bit.
  // Reproduced with GStreamer codecparsers 1.28.8: [0x13, 0] returns
  // BITSTREAM_ERROR, while [0x12, 0] succeeds, even without this crate.
  // The bridge must not duplicate that rejection. Keep the acceptance
  // assertion so updating the native dependency verifies the contract;
  // do not skip this case or rewrite the original bytes for the
  // library.
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let mut parser = Validator::new().unwrap();
  let mut bytes = &stream[32..];
  while !bytes.is_empty() {
    let size =
      u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let mut packet = bytes[12..12 + size].to_vec();
    packet[0] |= 1;
    assert_eq!(parser.validate(&packet, 32).unwrap().units.len(), 1);
    bytes = &bytes[12 + size..];
  }
}

#[test]
fn test_validate_ignores_obu_extension_reserved_bits() {
  // AV1 6.2.3 also requires extension_header_reserved_3bits to be
  // ignored. GStreamer codecparsers 1.28.8 rejects [0x16, 7, 0]
  // directly with BITSTREAM_ERROR before semantic admission. External
  // failure; preserve the normative acceptance assertion without
  // patching it.
  let admission =
    Validator::new().unwrap().validate(&[0x16, 7, 0], 0).unwrap();
  assert!(admission.units.is_empty());
}

#[test]
fn test_validate_metadata_reports_decoder_configuration_without_a_picture()
 {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let mut parser = Validator::new().unwrap();
  let admission = parser.validate(&stream[44..59], 0).unwrap();
  assert!(admission.units.is_empty());
  assert!(admission.configures_decoder);
  let padding = parser.validate(&[0x7a, 0], 0).unwrap();
  assert!(padding.units.is_empty());
  assert!(!padding.configures_decoder);
}

#[test]
fn test_validate_configuration_belongs_to_the_following_coded_task() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let packet = &stream[44..44 + size];
  let mut parser = Validator::new().unwrap();
  let mut combined = packet.to_vec();
  combined.extend_from_slice(&packet[15..]);
  let units = parser.validate(&combined, 32).unwrap();
  assert_eq!(units.units.len(), 2);
  assert!(units.units[0].configures_decoder);
  assert!(!units.units[1].configures_decoder);
  let mut trailing = packet[15..].to_vec();
  trailing.extend_from_slice(&packet[..15]);
  let units = parser.validate(&trailing, 32).unwrap();
  assert_eq!(units.units.len(), 1);
  assert!(!units.units[0].configures_decoder);
}

#[test]
fn test_validate_preserves_sequence_state_and_recovers_at_keyframe() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  for damaged in [&[][..], &[0xff; 16][..], &[0x32, 1, 0xff][..]] {
    let mut parser = Validator::new().unwrap();
    for _ in 0..2 {
      assert!(parser.validate(damaged, 32).is_err());
      let mut bytes = &stream[32..];
      while !bytes.is_empty() {
        let size =
          u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        parser.validate(&bytes[12..12 + size], 32).unwrap();
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
  assert!(first.validate(&[0x32, 1, 0xff], 32).is_err());
  assert!(second.validate(&[0x32, 1, 0xff], 32).is_err());
}

#[test]
fn test_validate_checks_obu_lengths_extension_and_metadata_payloads() {
  for bytes in [
    vec![0x12, 0x80],
    vec![0x12, 3, 1],
    vec![0x12, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x80],
    vec![0x2a, 1, 1],
  ] {
    assert!(
      Validator::new().unwrap().validate(&bytes, 32).is_err(),
      "{bytes:x?}"
    );
  }
  for bytes in [vec![0x12, 0], vec![0x7a, 0], vec![0x4a, 0]] {
    Validator::new().unwrap().validate(&bytes, 32).unwrap();
  }
}

#[test]
fn test_validate_keyframe_recovers_without_repeating_sequence_header() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let packet = &stream[44..44 + size];
  let mut offset = 0;
  let mut key = Vec::new();
  // Fixture manipulation only: omit sequence OBU (type 1), keeping
  // complete OBU boundaries in this low-overhead keyframe packet.
  while offset < packet.len() {
    let start = offset;
    let header = packet[offset];
    assert_ne!(header & 2, 0);
    offset += 1 + usize::from(header & 4 != 0);
    let mut length = 0usize;
    for shift in (0..56).step_by(7) {
      let byte = packet[offset];
      offset += 1;
      length |= usize::from(byte & 127) << shift;
      if byte & 128 == 0 {
        break;
      }
    }
    offset += length;
    if (header >> 3) & 15 != 1 {
      key.extend_from_slice(&packet[start..offset]);
    }
  }
  let mut parser = Validator::new().unwrap();
  assert!(parser.validate(&key, 32).is_err());
  parser.validate(packet, 32).unwrap();
  assert!(parser.validate(&[0x32, 1, 0xff], 32).is_err());
  parser.validate(&key, 32).unwrap();
}

#[test]
fn test_validate_framed_packet_cannot_end_with_header_but_no_tiles() {
  // Sequence + valid standalone keyframe header from av1-yuv420p.ivf,
  // with AV1 trailing_bits. No tile-group OBU follows. The V4L2 format
  // is framed (not CONTINUOUS_BYTESTREAM), so this input is incomplete.

  assert!(
    Validator::new()
      .unwrap()
      .validate(FRAME_HEADER_WITH_SEQUENCE, 32)
      .is_err()
  );
}

const FRAME_HEADER_WITH_SEQUENCE: &[u8] = &[
  0x0a, 0x0b, 0, 0, 0, 0xf9, 0xdb, 0xff, 0xf8, 0x84, 0x2a, 2, 0x80,
  0x1a, 0x1b, 0x10, 2, 0x1a, 0xc5, 0x63, 0x60, 0xbd, 0xfc, 0x1f, 0xd8,
  7, 0xfc, 2, 0x10, 1, 0x12, 0, 0, 0, 0, 0, 4, 0x10, 0x41, 0, 8, 0x23,
];

#[test]
fn test_validate_redundant_header_requires_an_unfinished_primary_header()
 {
  let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
  packet[13] = 0x3a; // REDUNDANT_FRAME_HEADER in place of FRAME_HEADER.
  assert!(Validator::new().unwrap().validate(&packet, 32).is_err());
}

#[test]
fn test_validate_primary_header_cannot_replace_an_unfinished_picture() {
  let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
  packet.extend_from_slice(&FRAME_HEADER_WITH_SEQUENCE[13..]);
  packet.extend_from_slice(&tile_obu());
  assert!(Validator::new().unwrap().validate(&packet, 32).is_err());
}

#[test]
fn test_validate_redundant_header_must_match_the_primary_payload() {
  for bit in 0..8 {
    let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
    let mut redundant = FRAME_HEADER_WITH_SEQUENCE[13..].to_vec();
    redundant[0] = 0x3a;
    redundant[3] ^= 1 << bit;
    packet.extend_from_slice(&redundant);
    packet.extend_from_slice(&tile_obu());
    assert!(
      Validator::new().unwrap().validate(&packet, 32).is_err(),
      "changed bit {bit}"
    );
  }
}

#[test]
fn test_validate_identical_redundant_header_preserves_one_picture() {
  let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
  let mut redundant = FRAME_HEADER_WITH_SEQUENCE[13..].to_vec();
  redundant[0] = 0x3a;
  packet.extend_from_slice(&redundant);
  packet.extend_from_slice(&tile_obu());
  let units = Validator::new().unwrap().validate(&packet, 32).unwrap();
  assert_eq!(units.units.len(), 1);
  assert_eq!(units.units[0].range, 0..packet.len());
}

#[test]
fn test_validate_picture_obus_require_the_primary_layer_identity() {
  for layer in [0x08, 0x20] {
    let mut redundant = FRAME_HEADER_WITH_SEQUENCE[13..].to_vec();
    redundant[0] = 0x3e;
    redundant.insert(1, layer);
    let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
    packet.extend_from_slice(&redundant);
    packet.extend_from_slice(&tile_obu());
    assert!(Validator::new().unwrap().validate(&packet, 32).is_err());

    let mut tiles = tile_obu();
    tiles[0] = 0x26;
    tiles.insert(1, layer);
    let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
    packet.extend_from_slice(&tiles);
    assert!(Validator::new().unwrap().validate(&packet, 32).is_err());
  }
}

#[test]
fn test_validate_accepts_separate_header_and_tile_obus_in_one_framed_packet()
 {
  let mut packet = FRAME_HEADER_WITH_SEQUENCE.to_vec();
  packet.extend_from_slice(&tile_obu());
  Validator::new().unwrap().validate(&packet, 32).unwrap();
}

fn tile_obu() -> Vec<u8> {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  // The fixture starts with delimiter (2 bytes), sequence (13 bytes),
  // then a frame OBU. Its 27-byte uncompressed header is reproduced
  // above.
  let mut offset = 44 + 2 + 13;
  assert_eq!(stream[offset], 0x32);
  offset += 1;
  while stream[offset] & 128 != 0 {
    offset += 1;
  }
  offset += 1;
  let tiles = &stream[offset + 27..44 + size];
  let mut packet = vec![0x22];
  let mut length = tiles.len();
  while length >= 128 {
    packet.push((length as u8 & 127) | 128);
    length >>= 7;
  }
  packet.push(length as u8);
  packet.extend_from_slice(tiles);
  packet
}

#[test]
fn test_validate_frame_obu_requires_implicit_complete_tile_group() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/av1-two-tiles.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  Validator::new().unwrap().validate(key, 32).unwrap();
  let mut packet = key.to_vec();
  // The frame OBU's two-tile group starts at packet byte 25. 10100000
  // signals explicit range [0,1]. This is allowed in a TILE_GROUP OBU,
  // but AV1 5.10 requires the flag to be zero inside a FRAME OBU.
  assert_eq!(packet[25], 0);
  packet[25] = 0xa0;
  assert!(Validator::new().unwrap().validate(&packet, 32).is_err());
}
