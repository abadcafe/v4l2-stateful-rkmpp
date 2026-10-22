//! module: vp9::native_tests; responsibility: specify native VP9 state
//! ownership through its public interface. operations: tests; entities:
//! none; tests: superframe bounds, owned facts and independent owners.
use super::native::Parser;

fn keyframe() -> &'static [u8] {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp9-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  &stream[44..44 + size]
}

#[test]
fn test_index_describes_every_superframe_extent() {
  let key = keyframe();
  let mut packet = [key, key].concat();
  packet.push(0xc9);
  for _ in 0..2 {
    packet.extend_from_slice(
      &u16::try_from(key.len()).unwrap().to_le_bytes(),
    );
  }
  packet.push(0xc9);
  let mut parser = Parser::new().unwrap();
  let index = parser.index(&packet).unwrap();
  packet.fill(0);
  drop(packet);
  assert_eq!(index.count, 2);
  assert_eq!(index.index_bytes, 6);
  assert_eq!(&index.sizes[..2], &[key.len() as u32; 2]);
  assert!(parser.index(&[0xc9]).is_err());
  let single = parser.index(key).unwrap();
  assert_eq!((single.count, single.index_bytes), (1, 0));
  assert_eq!(single.sizes[0], key.len() as u32);
}

#[test]
fn test_frame_detaches_key_and_existing_reference_facts() {
  let mut parser = Parser::new().unwrap();
  let frame = parser.frame(keyframe()).unwrap();
  assert!(frame.key_frame && frame.display && !frame.existing);
  assert!(frame.header_bytes > 0 && frame.partition_bytes > 0);
  let existing = parser.frame(&[0x88]).unwrap();
  assert!(existing.existing && existing.display);
  assert!(!existing.key_frame);
  assert!(parser.frame(&[0x82]).is_err());
  assert!(parser.frame(keyframe()).unwrap().key_frame);
}

#[test]
fn test_new_owns_independent_native_state_until_drop() {
  let mut first = Parser::new().unwrap();
  let mut second = Parser::new().unwrap();
  for _ in 0..2 {
    assert!(first.frame(keyframe()).is_ok());
    assert!(second.frame(keyframe()).is_ok());
  }
  drop(first);
  assert!(second.frame(&[0x88]).unwrap().existing);
}
