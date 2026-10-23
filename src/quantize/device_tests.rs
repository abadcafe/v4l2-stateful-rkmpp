//! module: quantize::device_tests; responsibility: verify hardware
//! device admission.
//! operations: test entrypoints; entities: none; tests:
//! test_new_accepts_dma_buf_memory_on_hardware_device.
use std::os::fd::AsFd;

use dma_buf::memory::Buffer;

use super::device::Device;
#[test]
fn test_new_accepts_dma_buf_memory_on_hardware_device() {
  let device = Device::new().unwrap();
  let buffer = Buffer::allocate(4096).unwrap();
  assert_ne!(device.memory_bits(buffer.as_fd()).unwrap(), 0);
}
