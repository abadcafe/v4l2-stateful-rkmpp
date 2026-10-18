//! module: quantize::imports_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_new_rejects_budget_that_cannot_retain_both_conversion_images
//! test_pair_reuses_dma_identity_across_duplicated_file_descriptors
//! test_pair_eviction_preserves_both_live_conversion_descriptors
//! test_pair_rejects_non_dma_and_empty_imports_without_poisoning_cache
use std::num::NonZeroUsize;
use std::os::fd::AsFd;

use super::context::Context;
use super::imports::Cache;
use crate::dma::Buffer;
use crate::image::Plane;

#[test]
fn test_new_rejects_budget_that_cannot_retain_both_conversion_images() {
  assert!(
    Cache::new(
      Context::new().unwrap().device(),
      NonZeroUsize::new(1).unwrap()
    )
    .is_err()
  );
}

#[test]
fn test_pair_reuses_dma_identity_across_duplicated_file_descriptors() {
  let context = Context::new().unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let destination = Buffer::allocate(4096).unwrap();
  let first = cache
    .pair(
      Plane::new(source.as_fd(), 32, 32),
      Plane::new(destination.as_fd(), 32, 32),
    )
    .unwrap();
  let duplicate = source.as_fd().try_clone_to_owned().unwrap();
  let second = cache
    .pair(
      Plane::new(duplicate.as_fd(), 32, 32),
      Plane::new(destination.as_fd(), 32, 32),
    )
    .unwrap();
  for index in 0..2 {
    assert_eq!(
      first.descriptors(&context.device()).unwrap()[index].buffer,
      second.descriptors(&context.device()).unwrap()[index].buffer
    );
    assert_eq!(
      second.descriptors(&context.device()).unwrap()[index].range,
      4096
    );
  }
}

#[test]
fn test_pair_eviction_preserves_both_live_conversion_descriptors() {
  let context = Context::new().unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffers: Vec<_> =
    (0..3).map(|_| Buffer::allocate(4096).unwrap()).collect();
  let first = cache
    .pair(
      Plane::new(buffers[0].as_fd(), 32, 32),
      Plane::new(buffers[1].as_fd(), 32, 32),
    )
    .unwrap();
  let replaced = cache
    .pair(
      Plane::new(buffers[0].as_fd(), 32, 32),
      Plane::new(buffers[2].as_fd(), 32, 32),
    )
    .unwrap();
  assert_eq!(
    first.descriptors(&context.device()).unwrap()[0].buffer,
    replaced.descriptors(&context.device()).unwrap()[0].buffer
  );
  assert_ne!(
    replaced.descriptors(&context.device()).unwrap()[0].buffer,
    replaced.descriptors(&context.device()).unwrap()[1].buffer
  );
  let again = cache
    .pair(
      Plane::new(buffers[0].as_fd(), 32, 32),
      Plane::new(buffers[2].as_fd(), 32, 32),
    )
    .unwrap();
  assert_eq!(
    again.descriptors(&context.device()).unwrap()[0].buffer,
    replaced.descriptors(&context.device()).unwrap()[0].buffer
  );
  assert_eq!(
    again.descriptors(&context.device()).unwrap()[1].buffer,
    replaced.descriptors(&context.device()).unwrap()[1].buffer
  );
}

#[test]
fn test_pair_rejects_non_dma_and_empty_imports_without_poisoning_cache()
{
  let context = Context::new().unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let file = std::fs::File::open("/dev/null").unwrap();
  assert!(
    cache
      .pair(
        Plane::new(file.as_fd(), 32, 32),
        Plane::new(file.as_fd(), 32, 32)
      )
      .is_err()
  );
  let a = Buffer::allocate(4096).unwrap();
  let b = Buffer::allocate(4096).unwrap();
  let recovered = cache
    .pair(Plane::new(a.as_fd(), 32, 32), Plane::new(b.as_fd(), 32, 32))
    .unwrap();
  assert_ne!(
    recovered.descriptors(&context.device()).unwrap()[0].buffer,
    recovered.descriptors(&context.device()).unwrap()[1].buffer
  );
}

#[test]
fn test_pair_rejects_aliased_images_and_non_dma_source() {
  let context = Context::new().unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffer = Buffer::allocate(4096).unwrap();
  let duplicate = buffer.as_fd().try_clone_to_owned().unwrap();
  assert!(
    cache
      .pair(
        Plane::new(buffer.as_fd(), 32, 32),
        Plane::new(duplicate.as_fd(), 32, 32)
      )
      .is_err()
  );
  let invalid = std::fs::File::open("/dev/null").unwrap();
  assert!(
    cache
      .pair(
        Plane::new(invalid.as_fd(), 32, 32),
        Plane::new(buffer.as_fd(), 32, 32)
      )
      .is_err()
  );
}
