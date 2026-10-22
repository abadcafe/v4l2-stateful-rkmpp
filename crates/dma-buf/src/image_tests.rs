//! module: image_tests; responsibility: define the corresponding public
//! module contract. operations: Rust test entrypoints; entities: none;
//! tests: descriptor/layout views, retained-source caching, descriptor
//! selection and concurrent cache initialization.
use std::os::fd::{AsFd, AsRawFd};

use crate::image::{Plane, PlaneCache};
use crate::memory::Buffer;

#[test]
fn test_new_borrows_descriptor_with_explicit_layout() {
  let buffer = Buffer::allocate(4096).unwrap();
  let plane = Plane::new(buffer.as_fd(), 64, 32);
  assert_eq!((plane.stride(), plane.rows()), (64, 32));
  assert_eq!(plane.fd().as_raw_fd(), buffer.as_fd().as_raw_fd());
  assert_eq!(
    plane.metadata().unwrap().identity(),
    buffer.metadata().identity()
  );
  assert_eq!(plane.metadata().unwrap().size(), buffer.size() as i64);
}

#[test]
fn test_from_buffer_borrows_registered_storage_and_layout() {
  use std::os::fd::{AsFd, AsRawFd};
  let buffer = crate::memory::Buffer::allocate(4096).unwrap();
  let plane = crate::image::Plane::from_buffer(&buffer, 64, 32);
  assert_eq!(plane.fd().as_raw_fd(), buffer.as_fd().as_raw_fd());
  assert_eq!((plane.stride(), plane.rows()), (64, 32));
  assert_eq!(plane.metadata().unwrap().size(), 4096);
}

#[test]
fn test_plane_cache_owns_each_metadata_source_without_descriptor_duplication()
 {
  let first = Buffer::allocate(4096).unwrap();
  let second = Buffer::allocate(8192).unwrap();
  let first_fd = first.as_fd().as_raw_fd();
  let second_fd = second.as_fd().as_raw_fd();
  let first = PlaneCache::new(first);
  let second = PlaneCache::new(second);
  for _ in 0..3 {
    let first_plane = first.plane(64, 32).unwrap();
    let second_plane = second.plane(128, 32).unwrap();
    assert_eq!(first_plane.fd().as_raw_fd(), first_fd);
    assert_eq!(second_plane.fd().as_raw_fd(), second_fd);
    let first_metadata = first_plane.metadata().unwrap();
    let second_metadata = second_plane.metadata().unwrap();
    assert_eq!(first_metadata.size(), 4096);
    assert_eq!(second_metadata.size(), 8192);
    assert_ne!(first_metadata.identity(), second_metadata.identity());
  }
}

#[test]
fn test_plane_cache_binds_metadata_to_selected_descriptor() {
  use std::cell::Cell;
  use std::os::fd::BorrowedFd;

  struct Source {
    buffers: [Buffer; 2],
    selected: Cell<usize>,
  }
  impl AsFd for Source {
    fn as_fd(&self) -> BorrowedFd<'_> {
      self.buffers[self.selected.get()].as_fd()
    }
  }
  let cache = PlaneCache::new(Source {
    buffers: [
      Buffer::allocate(4096).unwrap(),
      Buffer::allocate(8192).unwrap(),
    ],
    selected: Cell::new(0),
  });
  for selected in [0, 1, 0, 1] {
    cache.source().selected.set(selected);
    let plane = cache.plane(64, 32).unwrap();
    let expected = cache.source().buffers[selected].metadata();
    let actual = plane.metadata().unwrap();
    assert_eq!(actual.identity(), expected.identity());
    assert_eq!(actual.size(), expected.size());
    assert_eq!(
      plane.fd().as_raw_fd(),
      cache.source().buffers[selected].as_fd().as_raw_fd()
    );
  }
}

#[test]
fn test_plane_cache_concurrent_initialization_keeps_each_descriptor_metadata()
 {
  use std::os::fd::BorrowedFd;
  use std::sync::Barrier;
  use std::sync::atomic::{AtomicUsize, Ordering};

  struct Source {
    buffers: [Buffer; 2],
    selected: AtomicUsize,
  }
  impl AsFd for Source {
    fn as_fd(&self) -> BorrowedFd<'_> {
      let selected = self.selected.fetch_add(1, Ordering::Relaxed) % 2;
      self.buffers[selected].as_fd()
    }
  }
  let cache = PlaneCache::new(Source {
    buffers: [
      Buffer::allocate(4096).unwrap(),
      Buffer::allocate(8192).unwrap(),
    ],
    selected: AtomicUsize::new(0),
  });
  let rendezvous = Barrier::new(2);
  std::thread::scope(|scope| {
    let check = || {
      rendezvous.wait();
      let plane = cache.plane(64, 32).unwrap();
      let expected = Plane::new(plane.fd(), 64, 32).metadata().unwrap();
      let actual = plane.metadata().unwrap();
      assert_eq!(actual.identity(), expected.identity());
      assert_eq!(actual.size(), expected.size());
    };
    let first = scope.spawn(check);
    let second = scope.spawn(check);
    first.join().unwrap();
    second.join().unwrap();
  });
}
