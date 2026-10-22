//! module: quantize::imports_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_new_rejects_budget_that_cannot_retain_both_conversion_images
//! test_pair_reuses_dma_identity_across_duplicated_file_descriptors
//! test_pair_eviction_preserves_both_live_conversion_descriptors
//! test_pair_rejects_non_dma_and_empty_imports_without_poisoning_cache
//! test_pair_cache_miss_preserves_cached_destination
//! test_pair_failed_destination_preserves_existing_imports
//! test_pair_rejects_aliased_images_and_non_dma_source
//! test_reserve_access_conflict_preserves_existing_imports
//! test_reserve_failed_preparation_preserves_cache_and_releases_access
//! test_reserve_foreign_candidate_preserves_cache_and_access
//! test_reserve_outstanding_candidates_keep_one_cached_import_per_storage
use std::num::NonZeroUsize;
use std::os::fd::AsFd;

use dma_buf::image::Plane;
use dma_buf::memory::Buffer;

use super::context::Context;
use super::imports::{Cache, Pair};

#[test]
fn test_descriptors_binds_only_checked_image_ranges() {
  use dma_buf::image::Depth;

  use super::layout::Layout;
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(8192).unwrap();
  let source = Plane::from_buffer(&source, 32, 32);
  let target = Plane::from_buffer(&target, 64, 32);
  let pair = cache.pair(source, target).unwrap();
  let reservation = cache.reserve(pair, || Ok(())).unwrap().0;
  let layout =
    Layout::new(Depth::Eight, 32, 32, source, target).unwrap();
  let buffers =
    reservation.descriptors(&context.device(), layout).unwrap();
  assert_eq!(buffers.map(|buffer| buffer.range), [1536, 3072]);
  let too_large = Layout::new(
    Depth::Eight,
    32,
    32,
    Plane::new(source.fd(), 128, 32),
    target,
  )
  .unwrap();
  assert_eq!(
    reservation
      .descriptors(&context.device(), too_large)
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::InvalidInput
  );
}

// Admit through the public reservation boundary, then retain a fresh
// candidate so cache eviction cannot recycle the observed Vulkan
// handles.
fn retained(
  cache: &mut Cache,
  source: Plane<'_>,
  destination: Plane<'_>,
) -> Pair {
  let pair = cache.pair(source, destination).unwrap();
  drop(cache.reserve(pair, || Ok(())).unwrap());
  cache.pair(source, destination).unwrap()
}

#[test]
fn test_new_rejects_budget_that_cannot_retain_both_conversion_images() {
  assert!(
    Cache::new(
      Context::new(&super::Factory::default()).unwrap().device(),
      NonZeroUsize::new(1).unwrap()
    )
    .is_err()
  );
}

#[test]
fn test_pair_reuses_dma_identity_across_duplicated_file_descriptors() {
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let destination = Buffer::allocate(4096).unwrap();
  let first = retained(
    &mut cache,
    Plane::new(source.as_fd(), 32, 32),
    Plane::new(destination.as_fd(), 32, 32),
  );
  let duplicate = source.as_fd().try_clone_to_owned().unwrap();
  let second = retained(
    &mut cache,
    Plane::new(duplicate.as_fd(), 32, 32),
    Plane::new(destination.as_fd(), 32, 32),
  );
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
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffers: Vec<_> =
    (0..3).map(|_| Buffer::allocate(4096).unwrap()).collect();
  let first = retained(
    &mut cache,
    Plane::new(buffers[0].as_fd(), 32, 32),
    Plane::new(buffers[1].as_fd(), 32, 32),
  );
  let replaced = retained(
    &mut cache,
    Plane::new(buffers[0].as_fd(), 32, 32),
    Plane::new(buffers[2].as_fd(), 32, 32),
  );
  assert_eq!(
    first.descriptors(&context.device()).unwrap()[0].buffer,
    replaced.descriptors(&context.device()).unwrap()[0].buffer
  );
  assert_ne!(
    replaced.descriptors(&context.device()).unwrap()[0].buffer,
    replaced.descriptors(&context.device()).unwrap()[1].buffer
  );
  let again = retained(
    &mut cache,
    Plane::new(buffers[0].as_fd(), 32, 32),
    Plane::new(buffers[2].as_fd(), 32, 32),
  );
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
  let context = Context::new(&super::Factory::default()).unwrap();
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
  let recovered = retained(
    &mut cache,
    Plane::new(a.as_fd(), 32, 32),
    Plane::new(b.as_fd(), 32, 32),
  );
  assert_ne!(
    recovered.descriptors(&context.device()).unwrap()[0].buffer,
    recovered.descriptors(&context.device()).unwrap()[1].buffer
  );
}

#[test]
fn test_pair_rejects_aliased_images_and_non_dma_source() {
  let context = Context::new(&super::Factory::default()).unwrap();
  let cache =
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

#[test]
fn test_pair_cache_miss_preserves_cached_destination() {
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffers: Vec<_> =
    (0..3).map(|_| Buffer::allocate(4096).unwrap()).collect();
  let first = retained(
    &mut cache,
    Plane::from_buffer(&buffers[0], 32, 32),
    Plane::from_buffer(&buffers[1], 32, 32),
  );
  let second = retained(
    &mut cache,
    Plane::from_buffer(&buffers[2], 32, 32),
    Plane::from_buffer(&buffers[0], 32, 32),
  );
  assert_eq!(
    first.descriptors(&context.device()).unwrap()[0].buffer,
    second.descriptors(&context.device()).unwrap()[1].buffer
  );
}

#[test]
fn test_pair_failed_destination_preserves_existing_imports() {
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffers: Vec<_> =
    (0..3).map(|_| Buffer::allocate(4096).unwrap()).collect();
  let first = retained(
    &mut cache,
    Plane::from_buffer(&buffers[0], 32, 32),
    Plane::from_buffer(&buffers[1], 32, 32),
  );
  let empty = std::fs::File::open("/dev/null").unwrap();
  assert!(
    cache
      .pair(
        Plane::from_buffer(&buffers[2], 32, 32),
        Plane::new(empty.as_fd(), 32, 32),
      )
      .is_err()
  );
  let recovered = retained(
    &mut cache,
    Plane::from_buffer(&buffers[0], 32, 32),
    Plane::from_buffer(&buffers[1], 32, 32),
  );
  for index in 0..2 {
    assert_eq!(
      first.descriptors(&context.device()).unwrap()[index].buffer,
      recovered.descriptors(&context.device()).unwrap()[index].buffer
    );
  }
}

#[test]
fn test_reserve_access_conflict_preserves_existing_imports() {
  let context = Context::new(&super::Factory::default()).unwrap();
  for busy in 2..4 {
    let mut cache =
      Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
        .unwrap();
    let buffers: Vec<_> =
      (0..4).map(|_| Buffer::allocate(4096).unwrap()).collect();
    let existing = retained(
      &mut cache,
      Plane::from_buffer(&buffers[0], 32, 32),
      Plane::from_buffer(&buffers[1], 32, 32),
    );
    let access = buffers[busy].write().unwrap();
    let candidate = cache
      .pair(
        Plane::from_buffer(&buffers[2], 32, 32),
        Plane::from_buffer(&buffers[3], 32, 32),
      )
      .unwrap();
    let mut prepared = false;
    let rejected = cache.reserve(candidate, || {
      prepared = true;
      Ok(())
    });
    assert!(!prepared);
    assert_eq!(
      rejected.err().unwrap().kind(),
      std::io::ErrorKind::WouldBlock
    );
    buffers[5 - busy].write().unwrap().finish().unwrap();
    let recovered = retained(
      &mut cache,
      Plane::from_buffer(&buffers[0], 32, 32),
      Plane::from_buffer(&buffers[1], 32, 32),
    );
    for index in 0..2 {
      assert_eq!(
        existing.descriptors(&context.device()).unwrap()[index].buffer,
        recovered.descriptors(&context.device()).unwrap()[index].buffer
      );
    }
    access.finish().unwrap();
  }
}

#[test]
fn test_reserve_outstanding_candidates_keep_one_cached_import_per_storage()
 {
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(4).unwrap())
      .unwrap();
  let a = Buffer::allocate(4096).unwrap();
  let b = Buffer::allocate(4096).unwrap();
  let source = Plane::from_buffer(&a, 32, 32);
  let destination = Plane::from_buffer(&b, 32, 32);
  let first = cache.pair(source, destination).unwrap();
  let second = cache.pair(source, destination).unwrap();
  let expected = second.descriptors(&context.device()).unwrap();
  drop(cache.reserve(first, || Ok(())).unwrap());
  drop(cache.reserve(second, || Ok(())).unwrap());
  let current = cache.pair(source, destination).unwrap();
  for (actual, expected) in
    current.descriptors(&context.device()).unwrap().iter().zip(expected)
  {
    assert_eq!(actual.buffer, expected.buffer);
  }
}

#[test]
fn test_reserve_foreign_candidate_preserves_cache_and_access() {
  let context = Context::new(&super::Factory::default()).unwrap();
  let other = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let foreign =
    Cache::new(other.device(), NonZeroUsize::new(2).unwrap()).unwrap();
  let a = Buffer::allocate(4096).unwrap();
  let b = Buffer::allocate(4096).unwrap();
  let source = Plane::from_buffer(&a, 32, 32);
  let destination = Plane::from_buffer(&b, 32, 32);
  let existing = retained(&mut cache, source, destination);
  let candidate = foreign.pair(source, destination).unwrap();
  let mut prepared = false;
  let result = cache.reserve(candidate, || {
    prepared = true;
    Ok(())
  });
  assert!(!prepared);
  assert_eq!(
    result.err().unwrap().kind(),
    std::io::ErrorKind::InvalidInput
  );
  a.write().unwrap().finish().unwrap();
  b.write().unwrap().finish().unwrap();
  let current = retained(&mut cache, source, destination);
  for index in 0..2 {
    assert_eq!(
      current.descriptors(&context.device()).unwrap()[index].buffer,
      existing.descriptors(&context.device()).unwrap()[index].buffer
    );
  }
}

#[test]
fn test_reserve_failed_preparation_preserves_cache_and_releases_access()
{
  let context = Context::new(&super::Factory::default()).unwrap();
  let mut cache =
    Cache::new(context.device(), NonZeroUsize::new(2).unwrap())
      .unwrap();
  let buffers: Vec<_> =
    (0..4).map(|_| Buffer::allocate(4096).unwrap()).collect();
  let existing = retained(
    &mut cache,
    Plane::from_buffer(&buffers[0], 32, 32),
    Plane::from_buffer(&buffers[1], 32, 32),
  );
  let candidate = cache
    .pair(
      Plane::from_buffer(&buffers[2], 32, 32),
      Plane::from_buffer(&buffers[3], 32, 32),
    )
    .unwrap();
  let mut preparations = 0;
  let rejected = cache.reserve(candidate, || {
    preparations += 1;
    for buffer in &buffers[2..] {
      assert_eq!(
        buffer.read().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
      );
    }
    Err::<(), _>(std::io::Error::from_raw_os_error(libc::EMFILE))
  });
  assert_eq!(preparations, 1);
  assert_eq!(
    rejected.err().unwrap().raw_os_error(),
    Some(libc::EMFILE)
  );
  for buffer in &buffers[2..] {
    buffer.write().unwrap().finish().unwrap();
  }
  let current = retained(
    &mut cache,
    Plane::from_buffer(&buffers[0], 32, 32),
    Plane::from_buffer(&buffers[1], 32, 32),
  );
  for index in 0..2 {
    assert_eq!(
      current.descriptors(&context.device()).unwrap()[index].buffer,
      existing.descriptors(&context.device()).unwrap()[index].buffer,
    );
  }
}
