//! module: mpp::packet_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_eos_prepare_rejects_input_lease
//! test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop
//! test_input_packet_and_lease_retain_owned_mapping
use super::packet::Packet;
use crate::dma::Buffer;

#[test]
fn test_eos_prepare_rejects_input_lease() {
  let mut packet = Packet::eos().unwrap();
  assert_eq!(
    packet.prepare(1, 0).err().unwrap().kind(),
    std::io::ErrorKind::InvalidInput
  );
}

#[test]
fn test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut packet = Packet::input(buffer.clone()).unwrap();
  assert!(packet.prepare(0, 0).is_err());
  assert!(packet.prepare(4097, 0).is_err());
  let access = packet.prepare(4096, 42).unwrap();
  assert!(buffer.read().is_err());
  assert!(packet.prepare(16, 43).is_err());
  access.finish().unwrap();
  let access = packet.prepare(16, 43).unwrap();
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_input_packet_and_lease_retain_owned_mapping() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut packet = Packet::input(buffer.clone()).unwrap();
  let access = packet.prepare(16, 0).unwrap();
  drop(packet);
  assert!(buffer.write().is_err());
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}
