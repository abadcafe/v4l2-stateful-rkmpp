//! module: quantize::layout_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_new_layout_exposes_checked_shader_parameters_and_required_sizes
//! test_new_layout_accepts_odd_extents_and_inclusive_limits
//! test_new_layout_rejects_empty_oversized_misaligned_and_undersized_views
use std::os::fd::AsFd;

use super::layout::Layout;
use crate::image::Plane;

#[test]
fn test_new_layout_exposes_checked_shader_parameters_and_required_sizes()
 {
  let file = std::fs::File::open("/dev/null").unwrap();
  let layout = Layout::new(
    32,
    32,
    Plane::new(file.as_fd(), 64, 32),
    Plane::new(file.as_fd(), 32, 32),
  )
  .unwrap();
  assert_eq!(layout.parameters(), [32, 32, 64, 32, 32, 32]);
  assert_eq!(
    (layout.source_size(), layout.destination_size()),
    (3072, 1536)
  );
}

#[test]
fn test_new_layout_accepts_odd_extents_and_inclusive_limits() {
  let file = std::fs::File::open("/dev/null").unwrap();
  for (width, height, source_stride, destination_stride, rows) in
    [(255, 127, 320, 256, 128), (8192, 8192, 65536, 65536, 16384)]
  {
    Layout::new(
      width,
      height,
      Plane::new(file.as_fd(), source_stride, rows),
      Plane::new(file.as_fd(), destination_stride, rows),
    )
    .unwrap();
  }
}

#[test]
fn test_new_layout_rejects_empty_oversized_misaligned_and_undersized_views()
 {
  let file = std::fs::File::open("/dev/null").unwrap();
  for p in [
    [0, 32, 64, 32, 32, 32],
    [32, 0, 64, 32, 32, 32],
    [8193, 32, 65536, 32, 65536, 32],
    [32, 8193, 64, 16384, 32, 16384],
    [32, 32, 40, 31, 32, 32],
    [32, 32, 40, 32, 32, 31],
    [32, 32, 64, 16385, 32, 32],
    [32, 32, 64, 32, 32, 16385],
    [32, 32, 65540, 32, 32, 32],
    [32, 32, 64, 32, 65540, 32],
    [32, 32, 42, 32, 32, 32],
    [32, 32, 64, 32, 34, 32],
    [32, 32, 36, 32, 32, 32],
    [32, 32, 64, 32, 28, 32],
  ] {
    assert_eq!(
      Layout::new(
        p[0],
        p[1],
        Plane::new(file.as_fd(), p[2], p[3]),
        Plane::new(file.as_fd(), p[4], p[5])
      )
      .unwrap_err()
      .kind(),
      std::io::ErrorKind::InvalidInput
    );
  }
}
