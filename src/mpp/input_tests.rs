//! module: mpp::input_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_new_persistent_map_retains_allocation_and_reads_written_bytes
//! test_begin_read_rejects_overlapping_cpu_access
use super::input::InputMap;
use crate::dma::Buffer;

#[test]
fn test_new_persistent_map_retains_allocation_and_reads_written_bytes()
{
  let buffer = Buffer::allocate(4096).unwrap();
  buffer.write().unwrap()[0..4].copy_from_slice(b"test");
  let input = InputMap::new(buffer.clone()).unwrap();
  assert_eq!(input.size(), 4096);
  let access = input.begin_read().unwrap();
  assert_eq!(
    unsafe {
      std::slice::from_raw_parts(input.pointer().cast::<u8>(), 4)
    },
    b"test"
  );
  assert_eq!(
    buffer.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  access.finish().unwrap();
  drop(buffer);
  input.begin_read().unwrap().finish().unwrap();
}

#[test]
fn test_begin_read_rejects_overlapping_cpu_access() {
  let buffer = Buffer::allocate(4096).unwrap();
  let input = InputMap::new(buffer.clone()).unwrap();
  let write = buffer.write().unwrap();
  assert!(input.begin_read().is_err());
  drop(write);
  input.begin_read().unwrap().finish().unwrap();
}
