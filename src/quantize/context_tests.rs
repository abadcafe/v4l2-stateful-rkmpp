//! module: quantize::context_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_dispatch_real_imports_completes_and_publishes_nv12_pixels
//! test_dispatch_rejects_undersized_imports_before_submission
//! test_dispatch_rejects_foreign_device_without_poisoning_context
//! test_dispatch_imports_survive_cache_eviction_before_submission
//! test_new_shared_factory_reuses_device_and_keeps_contexts_independent
use std::num::NonZeroUsize;
use std::os::fd::AsFd;

use dma_buf::image::{Depth, Plane};
use dma_buf::memory::Buffer;

use super::context::Context;
use super::imports::{Cache, Reservation};
use super::layout::Layout;

fn reserved(
  cache: &mut Cache,
  source: Plane<'_>,
  destination: Plane<'_>,
) -> Reservation {
  let pair = cache.pair(source, destination).unwrap();
  cache.reserve(pair, || Ok(())).unwrap().0
}

#[test]
fn test_dispatch_real_imports_completes_and_publishes_nv12_pixels() {
  let mut context = Context::new(&super::Factory::default()).unwrap();
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
  let buffers = reserved(
    &mut imports,
    Plane::new(source.as_fd(), 64, 32),
    Plane::new(target.as_fd(), 32, 32),
  );
  context.dispatch(buffers, layout).unwrap();
  assert_eq!(
    source.read().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert_eq!(
    target.write().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  let other_source = Buffer::allocate(4096).unwrap();
  let other_target = Buffer::allocate(4096).unwrap();
  assert_eq!(
    context
      .dispatch(
        reserved(
          &mut imports,
          Plane::new(other_source.as_fd(), 64, 32),
          Plane::new(other_target.as_fd(), 32, 32),
        ),
        layout
      )
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::WouldBlock
  );
  context.quiesce().unwrap();
  // Completion alone does not retire this context's resource owners.
  assert_eq!(
    target.read().err().unwrap().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert!(context.finish().unwrap());
  source.read().unwrap().finish().unwrap();
  target.read().unwrap().finish().unwrap();
  let buffers = reserved(
    &mut imports,
    Plane::new(source.as_fd(), 64, 32),
    Plane::new(target.as_fd(), 32, 32),
  );
  context.dispatch(buffers, layout).unwrap();
  drop(imports);
  context.quiesce().unwrap();
  assert!(context.finish().unwrap());
  assert!(target.read().unwrap()[..1536].iter().all(|byte| *byte == 1));
}

#[test]
fn test_dispatch_rejects_foreign_device_without_poisoning_context() {
  let mut context = Context::new(&super::Factory::default()).unwrap();
  let other = Context::new(&super::Factory::default()).unwrap();
  let mut imports =
    Cache::new(other.device(), NonZeroUsize::new(2).unwrap()).unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let a = Plane::new(source.as_fd(), 64, 32);
  let b = Plane::new(target.as_fd(), 32, 32);
  let layout = Layout::new(Depth::Ten, 32, 32, a, b).unwrap();
  assert_eq!(
    context
      .dispatch(reserved(&mut imports, a, b), layout)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
  assert!(context.finish().unwrap());
}

#[test]
fn test_dispatch_imports_survive_cache_eviction_before_submission() {
  let mut context = Context::new(&super::Factory::default()).unwrap();
  let mut imports =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  source.write().unwrap().fill(73);
  let a = Plane::new(source.as_fd(), 32, 32);
  let b = Plane::new(target.as_fd(), 32, 32);
  let buffers = reserved(&mut imports, a, b);
  let replacement_a = Buffer::allocate(4096).unwrap();
  let replacement_b = Buffer::allocate(4096).unwrap();
  drop(reserved(
    &mut imports,
    Plane::new(replacement_a.as_fd(), 32, 32),
    Plane::new(replacement_b.as_fd(), 32, 32),
  ));
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
  let mut context = Context::new(&super::Factory::default()).unwrap();
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
      .dispatch(reserved(&mut imports, a, b), layout)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
  assert!(context.finish().unwrap());
}

#[test]
fn test_new_shared_factory_reuses_device_and_keeps_contexts_independent()
 {
  let factory = super::Factory::default();
  let first = Context::new(&factory).unwrap();
  let second = Context::new(&factory.clone()).unwrap();
  assert!(std::sync::Arc::ptr_eq(&first.device(), &second.device()));
  drop(first);
  second.quiesce().unwrap();
  assert!(!std::sync::Arc::ptr_eq(
    &second.device(),
    &Context::new(&super::Factory::default()).unwrap().device()
  ));
}
