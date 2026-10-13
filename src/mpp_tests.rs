//! module: mpp_tests; responsibility: define the corresponding public
//! module contract. operations: Rust test entrypoints; entities: none;
//! tests:
//! test_new_unsupported_codec_fails_before_creating_decoder
//! test_eos_packet_rejects_input_preparation_through_facade
//! test_prepare_packet_bounds_and_empty_decoder_through_facade
use crate::dma::Buffer;
use crate::mpp::{Decoder, Packet};
use crate::notify::Notify;

#[test]
fn test_new_unsupported_codec_fails_before_creating_decoder() {
  assert!(Decoder::new(0, Notify::new().unwrap()).is_err());
}

#[test]
fn test_eos_packet_rejects_input_preparation_through_facade() {
  let mut packet = Packet::eos().unwrap();
  assert_eq!(
    packet.prepare(1, 0).err().unwrap().kind(),
    std::io::ErrorKind::InvalidInput
  );
}

#[test]
fn test_prepare_packet_bounds_and_empty_decoder_through_facade() {
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"H264"), Notify::new().unwrap())
      .unwrap();
  assert!(decoder.frame().unwrap().is_none());
  decoder.reset().unwrap();
  let mut packet =
    Packet::input(Buffer::allocate(4096).unwrap()).unwrap();
  assert!(packet.prepare(0, 0).is_err());
  assert!(packet.prepare(4097, 0).is_err());
  let access = packet.prepare(16, 7).unwrap();
  access.finish().unwrap();
  decoder.close().unwrap();
}
