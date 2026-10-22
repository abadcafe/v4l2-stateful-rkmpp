//! module: vp8::native_tests; responsibility: specify native VP8 state
//! ownership through its public interface. operations: tests; entities:
//! none; tests: frame facts, rejection, reset and independent owners.
use super::native::Parser;

fn packets() -> (&'static [u8], &'static [u8]) {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp8-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let next = 44 + size;
  let second =
    u32::from_le_bytes(stream[next..next + 4].try_into().unwrap())
      as usize;
  (&stream[44..44 + size], &stream[next + 12..next + 12 + second])
}

#[test]
fn test_frame_returns_owned_facts_without_retaining_input() {
  let (key, inter) = packets();
  let mut parser = Parser::new();
  let mut bytes = key.to_vec();
  let frame = parser.frame(&bytes).unwrap();
  bytes.fill(0);
  drop(bytes);
  assert!(frame.key_frame && frame.display);
  assert_eq!(
    frame.partition_bytes,
    u32::from_le_bytes([key[0], key[1], key[2], 0]) >> 5
  );
  assert!(frame.header_bits > 0);
  assert!(frame.header_bits <= frame.partition_bytes * 8);
  assert_eq!(
    (frame.width, frame.height),
    (
      u32::from(
        u16::from_le_bytes(key[6..8].try_into().unwrap()) & 0x3fff
      ),
      u32::from(
        u16::from_le_bytes(key[8..10].try_into().unwrap()) & 0x3fff
      )
    )
  );
  assert!(!parser.frame(inter).unwrap().key_frame);
}

#[test]
fn test_reset_recovers_rejected_input_and_isolates_owners() {
  let (key, inter) = packets();
  let mut first = Parser::new();
  let mut second = Parser::new();
  for parser in [&mut first, &mut second] {
    assert!(parser.frame(&[]).is_err());
    assert!(parser.frame(&key[..2]).is_err());
    parser.reset();
    assert!(parser.frame(key).unwrap().key_frame);
    assert!(parser.frame(inter).is_ok());
  }
  first.reset();
  drop(first);
  assert!(second.frame(inter).is_ok());
}
