//! module: quantize::context_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_dispatch_real_imports_completes_and_publishes_nv12_pixels
//! test_dispatch_rejects_null_and_undersized_descriptors_before_submission
use std::num::NonZeroUsize;
use std::os::fd::AsFd;

use ash::vk;

use super::context::Context;
use super::imports::Cache;
use super::layout::Layout;
use crate::dma::{Buffer, system};
use crate::image::{Depth, Plane};

#[test]
fn test_dispatch_real_imports_completes_and_publishes_nv12_pixels() {
  let mut context = Context::new().unwrap();
  let mut imports =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  {
    let mut bytes = source.write().unwrap();
    bytes.fill(0);
    for row in 0..48 {
      for column in 0..32 {
        let bit = column * 10;
        let offset = row * 64 + bit / 8;
        bytes[offset] |= (4u32 << (bit % 8)) as u8;
        bytes[offset + 1] |= (4u32 >> (8 - bit % 8)) as u8;
      }
    }
  }
  let layout = Layout::new(
    Depth::Ten,
    32,
    32,
    Plane::new(source.as_fd(), 64, 32),
    Plane::new(target.as_fd(), 32, 32),
  )
  .unwrap();
  let buffers = imports
    .pair(
      source.as_fd(),
      &system::metadata(source.as_fd()).unwrap(),
      target.as_fd(),
      &system::metadata(target.as_fd()).unwrap(),
    )
    .unwrap();
  context.dispatch(buffers, layout).unwrap();
  context.quiesce().unwrap();
  assert!(target.read().unwrap()[..1536].iter().all(|byte| *byte == 1));
}

#[test]
fn test_dispatch_rejects_null_and_undersized_descriptors_before_submission()
 {
  let mut context = Context::new().unwrap();
  let file = std::fs::File::open("/dev/null").unwrap();
  let layout = Layout::new(
    Depth::Ten,
    32,
    32,
    Plane::new(file.as_fd(), 64, 32),
    Plane::new(file.as_fd(), 32, 32),
  )
  .unwrap();
  assert_eq!(
    context
      .dispatch([vk::DescriptorBufferInfo::default(); 2], layout)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
  context.quiesce().unwrap();
}
