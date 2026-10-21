//! module: UAPI contract tests; responsibility: specify shared raw wire
//! values. operations: Rust test entrypoints; entities: none;
//! tests: POD representation, zero initialization, native integer bit
//! patterns.
use crate::wire::{Batch, Descriptor, Message, Pool};

#[test]
fn test_wire_records_are_pod_without_padding() {
  fn verify<T: bytemuck::Pod + Default>(size: usize) {
    assert_eq!(std::mem::size_of::<T>(), size);
    assert_eq!(bytemuck::bytes_of(&T::default()), vec![0; size]);
  }
  verify::<Message>(120);
  verify::<Descriptor>(8);
  verify::<Pool>(288);
  verify::<Batch>(3920);
}

#[test]
fn test_raw_wire_values_do_not_apply_service_validation() {
  let message = Message {
    version: u32::MAX,
    kind: u32::MAX,
    epoch: u64::MAX,
    reserved: [7],
    ..Default::default()
  };
  let bytes = bytemuck::bytes_of(&message);
  assert_eq!(bytemuck::pod_read_unaligned::<Message>(bytes), message);
  assert_eq!(Descriptor { fd: -17, size: u32::MAX }.fd, -17);
}
