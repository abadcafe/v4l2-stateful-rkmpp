//! module: mpp::packet_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop
//! test_input_packet_and_lease_retain_owned_mapping
use super::packet::Packet;
use crate::dma::Buffer;
use crate::timestamp::Source;

#[test]
fn test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut packet = Packet::input(buffer.clone()).unwrap();
  assert!(packet.prepare(0, 0, 0, Source::End).is_err());
  assert!(packet.prepare(0, 4097, 0, Source::End).is_err());
  let access = packet.prepare(0, 4096, 42, Source::End).unwrap();
  assert!(buffer.read().is_err());
  assert!(packet.prepare(0, 16, 43, Source::End).is_err());
  access.finish().unwrap();
  let access = packet.prepare(0, 16, 43, Source::End).unwrap();
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_input_packet_and_lease_retain_owned_mapping() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut packet = Packet::input(buffer.clone()).unwrap();
  let access = packet.prepare(0, 16, 0, Source::End).unwrap();
  drop(packet);
  assert!(buffer.write().is_err());
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_prepare_validates_offset_and_length_without_overflow() {
  let mut packet =
    Packet::input(Buffer::allocate(4096).unwrap()).unwrap();
  assert!(packet.prepare(4096, 1, 0, Source::End).is_err());
  assert!(packet.prepare(usize::MAX, 2, 0, Source::End).is_err());
  assert!(packet.prepare(1, usize::MAX, 0, Source::End).is_err());
  packet.prepare(16, 4080, 0, Source::End).unwrap().finish().unwrap();
}

#[test]
fn test_owned_packet_keeps_payload_and_timestamp_storage_live() {
  let _packet =
    Packet::owned(vec![1, 2, 3, 4], 7, Source::End).unwrap();
  assert!(Packet::owned(Vec::new(), 0, Source::End).is_err());
}

#[test]
fn test_task_view_retains_cpu_read_lease_and_checks_subrange() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut input = Packet::input(buffer.clone()).unwrap();
  let access = input.prepare(16, 32, 7, Source::Start).unwrap();
  assert!(access.packet(0..33, 1, Source::End).is_err());
  assert!(access.packet(2..2, 1, Source::End).is_err());
  let task = access.packet(4..20, 1, Source::End).unwrap();
  drop(access);
  assert!(buffer.write().is_err());
  drop(task);
  buffer.write().unwrap().finish().unwrap();
}
