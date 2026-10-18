//! module: quantize::context_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_dispatch_real_imports_completes_and_publishes_nv12_pixels
//! test_dispatch_rejects_undersized_imports_before_submission
//! test_dispatch_rejects_foreign_device_without_poisoning_context
//! test_dispatch_imports_survive_cache_eviction_before_submission
use std::num::NonZeroUsize;
use std::os::fd::AsFd;

use super::context::Context;
use super::imports::Cache;
use super::layout::Layout;
use crate::dma::Buffer;
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
      Plane::new(source.as_fd(), 64, 32),
      Plane::new(target.as_fd(), 32, 32),
    )
    .unwrap();
  context.dispatch(buffers, layout).unwrap();
  assert_eq!(
    context
      .dispatch(
        imports
          .pair(
            Plane::new(source.as_fd(), 64, 32),
            Plane::new(target.as_fd(), 32, 32)
          )
          .unwrap(),
        layout
      )
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::WouldBlock
  );
  context.quiesce().unwrap();
  assert!(context.finish().unwrap());
  let buffers = imports
    .pair(
      Plane::new(source.as_fd(), 64, 32),
      Plane::new(target.as_fd(), 32, 32),
    )
    .unwrap();
  context.dispatch(buffers, layout).unwrap();
  drop(imports);
  context.quiesce().unwrap();
  assert!(context.finish().unwrap());
  assert!(target.read().unwrap()[..1536].iter().all(|byte| *byte == 1));
}

#[test]
fn test_dispatch_rejects_foreign_device_without_poisoning_context() {
  let mut context = Context::new().unwrap();
  let other = Context::new().unwrap();
  let mut imports =
    Cache::new(other.device(), NonZeroUsize::new(2).unwrap()).unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let a = Plane::new(source.as_fd(), 64, 32);
  let b = Plane::new(target.as_fd(), 32, 32);
  let layout = Layout::new(Depth::Ten, 32, 32, a, b).unwrap();
  assert_eq!(
    context
      .dispatch(imports.pair(a, b).unwrap(), layout)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
  assert!(context.finish().unwrap());
}

#[test]
fn test_dispatch_imports_survive_cache_eviction_before_submission() {
  let mut context = Context::new().unwrap();
  let mut imports =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  source.write().unwrap().fill(73);
  let a = Plane::new(source.as_fd(), 32, 32);
  let b = Plane::new(target.as_fd(), 32, 32);
  let buffers = imports.pair(a, b).unwrap();
  let replacement_a = Buffer::allocate(4096).unwrap();
  let replacement_b = Buffer::allocate(4096).unwrap();
  drop(
    imports
      .pair(
        Plane::new(replacement_a.as_fd(), 32, 32),
        Plane::new(replacement_b.as_fd(), 32, 32),
      )
      .unwrap(),
  );
  drop(imports);
  context
    .dispatch(buffers, Layout::new(Depth::Eight, 32, 32, a, b).unwrap())
    .unwrap();
  context.quiesce().unwrap();
  assert!(context.finish().unwrap());
  assert!(target.read().unwrap()[..1536].iter().all(|&b| b == 73));
}

#[test]
fn test_dispatch_rejects_undersized_imports_before_submission() {
  let mut context = Context::new().unwrap();
  let mut imports =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let a = Plane::new(source.as_fd(), 64, 64);
  let b = Plane::new(target.as_fd(), 64, 64);
  let layout = Layout::new(Depth::Eight, 64, 64, a, b).unwrap();
  assert_eq!(
    context
      .dispatch(imports.pair(a, b).unwrap(), layout)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
  assert!(context.finish().unwrap());
}
