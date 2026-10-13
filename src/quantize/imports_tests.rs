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
use crate::dma::{Buffer, system};

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
  let source_stat = system::metadata(source.as_fd()).unwrap();
  let target_stat = system::metadata(destination.as_fd()).unwrap();
  let first = cache
    .pair(
      source.as_fd(),
      &source_stat,
      destination.as_fd(),
      &target_stat,
    )
    .unwrap();
  let duplicate = source.as_fd().try_clone_to_owned().unwrap();
  let second = cache
    .pair(
      duplicate.as_fd(),
      &source_stat,
      destination.as_fd(),
      &target_stat,
    )
    .unwrap();
  for index in 0..2 {
    assert_eq!(first[index].buffer, second[index].buffer);
    assert_eq!(second[index].range, 4096);
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
  let metadata: Vec<_> = buffers
    .iter()
    .map(|b| system::metadata(b.as_fd()).unwrap())
    .collect();
  let first = cache
    .pair(
      buffers[0].as_fd(),
      &metadata[0],
      buffers[1].as_fd(),
      &metadata[1],
    )
    .unwrap();
  let replaced = cache
    .pair(
      buffers[0].as_fd(),
      &metadata[0],
      buffers[2].as_fd(),
      &metadata[2],
    )
    .unwrap();
  assert_eq!(first[0].buffer, replaced[0].buffer);
  assert_ne!(replaced[0].buffer, replaced[1].buffer);
  let again = cache
    .pair(
      buffers[0].as_fd(),
      &metadata[0],
      buffers[2].as_fd(),
      &metadata[2],
    )
    .unwrap();
  assert_eq!(again[0].buffer, replaced[0].buffer);
  assert_eq!(again[1].buffer, replaced[1].buffer);
}

#[test]
fn test_pair_rejects_non_dma_and_empty_imports_without_poisoning_cache()
{
  let context = Context::new().unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let file = std::fs::File::open("/dev/null").unwrap();
  let stat = system::metadata(file.as_fd()).unwrap();
  assert!(
    cache.pair(file.as_fd(), &stat, file.as_fd(), &stat).is_err()
  );
  let a = Buffer::allocate(4096).unwrap();
  let b = Buffer::allocate(4096).unwrap();
  let recovered = cache
    .pair(
      a.as_fd(),
      &system::metadata(a.as_fd()).unwrap(),
      b.as_fd(),
      &system::metadata(b.as_fd()).unwrap(),
    )
    .unwrap();
  assert_ne!(recovered[0].buffer, recovered[1].buffer);
}
