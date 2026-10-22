//! module: rkmpp::packet_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop
//! test_input_packet_and_lease_retain_owned_mapping
use dma_buf::memory::Buffer;

use super::packet::Input;
use crate::{MAX_PACKET_SIZE, Payload};

#[test]
fn test_prepare_checks_lengths_and_releases_lease_on_finish_or_drop() {
  let buffer = Buffer::allocate(4096).unwrap();
  let packet = Input::new(buffer.clone()).unwrap();
  assert!(packet.prepare(0, 0).is_err());
  assert!(packet.prepare(0, 4097).is_err());
  let access = packet.prepare(0, 4096).unwrap();
  assert!(buffer.read().is_err());
  assert!(packet.prepare(0, 16).is_err());
  access.finish().unwrap();
  let access = packet.prepare(0, 16).unwrap();
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_input_packet_and_lease_retain_owned_mapping() {
  let buffer = Buffer::allocate(4096).unwrap();
  let packet = Input::new(buffer.clone()).unwrap();
  let access = packet.prepare(0, 16).unwrap();
  drop(packet);
  assert!(buffer.write().is_err());
  drop(access);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_prepare_validates_offset_and_length_without_overflow() {
  let packet = Input::new(Buffer::allocate(4096).unwrap()).unwrap();
  assert!(packet.prepare(4096, 1).is_err());
  assert!(packet.prepare(usize::MAX, 2).is_err());
  assert!(packet.prepare(1, usize::MAX).is_err());
  packet.prepare(16, 4080).unwrap().finish().unwrap();
}

#[test]
fn test_task_view_retains_cpu_read_lease_and_checks_subrange() {
  let buffer = Buffer::allocate(4096).unwrap();
  let input = Input::new(buffer.clone()).unwrap();
  let access = input.prepare(16, 32).unwrap();
  assert!(access.packet(Payload::Input(0..33), 1, 0).is_err());
  assert!(access.packet(Payload::Input(2..2), 1, 0).is_err());
  let task = access.packet(Payload::Input(4..20), 1, 0).unwrap();
  drop(access);
  assert!(buffer.write().is_err());
  drop(task);
  buffer.write().unwrap().finish().unwrap();
}

#[test]
fn test_task_repeated_submissions_preserve_exclusive_mapping_until_last_task()
 {
  let buffer = Buffer::allocate(4096).unwrap();
  let input = Input::new(buffer.clone()).unwrap();
  for offset in 0..64 {
    let access = input.prepare(offset, 64).unwrap();
    let first =
      access.packet(Payload::Input(0..16), 1, 0x10000).unwrap();
    let second = access.packet(Payload::Input(16..64), 2, 0).unwrap();
    assert!(!first.consumed() && !second.consumed());
    drop(first);
    assert!(buffer.write().is_err());
    drop(second);
    access.finish().unwrap();
    buffer.write().unwrap().finish().unwrap();
  }
}

#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_packet_get_data(packet: *mut std::ffi::c_void) -> *const u8;
  fn mpp_packet_get_length(packet: *mut std::ffi::c_void) -> usize;
  fn mpp_packet_get_pts(packet: *mut std::ffi::c_void) -> i64;
  fn mpp_packet_get_dts(packet: *mut std::ffi::c_void) -> i64;
}
#[test]
fn test_packet_prefixed_assembly_preserves_data_without_retaining_input_lease()
 {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut write = buffer.write().unwrap();
  write[16..20].copy_from_slice(&[9, 3, 4, 8]);
  write.finish().unwrap();
  let input = Input::new(buffer.clone()).unwrap();
  let access = input.prepare(16, 4).unwrap();
  let packet = access
    .packet(
      Payload::Prefixed { prefix: vec![1, 2], range: 1..3 },
      7,
      0x10000,
    )
    .unwrap();
  access.finish().unwrap();
  let mut write = buffer.write().unwrap();
  write.fill(0);
  write.finish().unwrap();
  // Observe the public SDK packet interface while its owner retains
  // the complete storage; no private Packet fields are inspected.
  let (bytes, pts, dts) = unsafe {
    let handle = packet.handle();
    (
      std::slice::from_raw_parts(
        mpp_packet_get_data(handle),
        mpp_packet_get_length(handle),
      ),
      mpp_packet_get_pts(handle),
      mpp_packet_get_dts(handle),
    )
  };
  assert_eq!(bytes, [1, 2, 3, 4]);
  assert_eq!(pts, 7);
  assert_eq!(dts, 0x10000_i64);
}
#[test]
fn test_packet_prefixed_bounds_reject_without_losing_input_access() {
  let input = Input::new(Buffer::allocate(4096).unwrap()).unwrap();
  let access = input.prepare(0, 4).unwrap();
  for range in [0..0, 0..5, usize::MAX..usize::MAX] {
    assert!(
      access
        .packet(Payload::Prefixed { prefix: vec![1], range }, 0, 0)
        .is_err()
    );
  }
  assert!(
    access
      .packet(
        Payload::Prefixed {
          prefix: vec![1; MAX_PACKET_SIZE],
          range: 0..1
        },
        0,
        0
      )
      .is_err()
  );
  let packet = access
    .packet(
      Payload::Prefixed {
        prefix: vec![1; MAX_PACKET_SIZE - 1],
        range: 0..1,
      },
      0,
      0,
    )
    .unwrap();
  assert!(!packet.consumed());
  access.finish().unwrap();
}
