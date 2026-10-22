//! module: quantize_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_convert_pixels_saturation_fractional_mean_padding_and_odd_dimensions
//! test_convert_distinct_buffer_lifetimes_preserve_all_pixels
//! test_convert_4k_frame_preserves_pixels_and_padding
//! test_submit_cpu_owned_source_is_rejected_and_releases_target
//! test_submit_cpu_owned_target_alias_is_rejected_and_releases_source
//! test_submit_reserves_both_images_until_completion_or_cancellation
//! test_submit_independent_contexts_cannot_overlap_the_same_storage
//! test_submit_regular_file_images_reject_and_allow_retry
use std::os::fd::AsFd;

use dma_buf::image::{Depth, Plane};
use dma_buf::memory::Buffer;

use crate::quantize::Quantizer;

fn sample(x: u32, y: u32, plane: u32) -> u32 {
  ((x / 8) + (y / 8) * 31 + plane * 509) & 1023
}

fn check(q: &mut Quantizer, w: u32, h: u32, repeats: usize) {
  let rows = (h + 15) & !15;
  let ss = ((((w + 1) & !1) * 10 / 8) + 255) & !255;
  let ds = (w + 63) & !63;
  let source = Buffer::allocate((ss * rows * 3 / 2) as usize).unwrap();
  let destination =
    Buffer::allocate((ds * rows * 3 / 2) as usize).unwrap();
  {
    let mut bytes = source.write().unwrap();
    bytes.fill(0);
    for p in 0..2 {
      for y in 0..if p == 0 { h } else { h.div_ceil(2) } {
        for x in 0..if p == 0 { w } else { (w + 1) & !1 } {
          let v = sample(if p == 0 { x } else { x / 2 }, y, p);
          let bit = x * 10;
          let off = ((y + if p == 0 { 0 } else { rows }) * ss + bit / 8)
            as usize;
          bytes[off] |= (v << (bit % 8)) as u8;
          bytes[off + 1] |= (v >> (8 - bit % 8)) as u8;
        }
      }
    }
  }
  destination.write().unwrap().fill(0xa5);
  let source_plane = Plane::new(source.as_fd(), ss, rows);
  let destination_plane = Plane::new(destination.as_fd(), ds, rows);
  for _ in 0..repeats {
    q.convert(Depth::Ten, w, h, source_plane, destination_plane)
      .unwrap();
  }
  {
    let pixels = destination.read().unwrap();
    for p in 0..2 {
      for y in 0..if p == 0 { h } else { h.div_ceil(2) } {
        for x in 0..if p == 0 { w } else { (w + 1) & !1 } {
          let cx = if p == 0 { x } else { x / 2 };
          let threshold =
            [[0, 2], [3, 1]][(y % 2) as usize][(cx % 2) as usize];
          let expected =
            ((sample(cx, y, p) + threshold) / 4).min(255) as u8;
          assert_eq!(
            pixels
              [((y + if p == 0 { 0 } else { rows }) * ds + x) as usize],
            expected
          );
        }
      }
    }
    for p in 0..2 {
      let ph = if p == 0 { h } else { h.div_ceil(2) };
      let pw = if p == 0 { w } else { w / 2 };
      for y in (0..ph.saturating_sub(1)).step_by(8) {
        for x in (0..pw.saturating_sub(1)).step_by(8) {
          let mut sum = 0u32;
          for j in 0..2 {
            for i in 0..2 {
              sum += pixels[((y + j + if p == 0 { 0 } else { rows })
                * ds
                + if p == 0 { x + i } else { 2 * (x + i) })
                as usize] as u32;
            }
          }
          assert_eq!(sum, sample(x, y, p).min(1020));
        }
      }
    }
    for y in h..rows {
      assert!(
        pixels[(y * ds) as usize..((y + 1) * ds) as usize]
          .iter()
          .all(|x| *x == 0xa5)
      );
    }
  }
  assert!(
    q.convert(Depth::Ten, 0, h, source_plane, destination_plane)
      .is_err()
  );
  assert!(
    q.convert(
      Depth::Ten,
      w,
      h,
      Plane::new(source.as_fd(), 4, rows),
      destination_plane
    )
    .is_err()
  );
  assert!(
    q.convert(
      Depth::Ten,
      w,
      h,
      source_plane,
      Plane::new(source.as_fd(), ds, rows)
    )
    .is_err()
  );
  q.convert(Depth::Ten, w, h, source_plane, destination_plane).unwrap();
}

#[test]
fn test_convert_pixels_saturation_fractional_mean_padding_and_odd_dimensions()
 {
  let mut q =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  check(&mut q, 256, 256, 8);
  check(&mut q, 258, 130, 3);
  check(&mut q, 255, 127, 3);
}

#[test]
fn test_convert_distinct_buffer_lifetimes_preserve_all_pixels() {
  let mut q =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  for _ in 0..16 {
    check(&mut q, 32, 32, 1);
  }
}

#[test]
fn test_convert_4k_frame_preserves_pixels_and_padding() {
  check(
    &mut Quantizer::new(&crate::quantize::Factory::default()).unwrap(),
    3840,
    2160,
    2,
  );
}

#[test]
fn test_convert_eight_bit_preserves_pixels_across_different_strides() {
  let source = Buffer::allocate(128 * 64 * 3 / 2).unwrap();
  let target = Buffer::allocate(64 * 80 * 3 / 2).unwrap();
  {
    let mut bytes = source.write().unwrap();
    for (i, byte) in bytes.iter_mut().enumerate() {
      *byte = (i % 251) as u8;
    }
  }
  target.write().unwrap().fill(0xa5);
  Quantizer::new(&crate::quantize::Factory::default())
    .unwrap()
    .convert(
      Depth::Eight,
      64,
      64,
      Plane::new(source.as_fd(), 128, 64),
      Plane::new(target.as_fd(), 64, 80),
    )
    .unwrap();
  let original = source.read().unwrap();
  let copied = target.read().unwrap();
  for plane in 0..2 {
    for row in 0..if plane == 0 { 64 } else { 32 } {
      let src = (plane * 64 + row) * 128;
      let dst = (plane * 80 + row) * 64;
      assert_eq!(&copied[dst..dst + 64], &original[src..src + 64]);
    }
  }
  assert!(copied[64 * 64..64 * 80].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn test_convert_concurrent_contexts_publish_pixels_to_cpu_readers() {
  let factory = crate::quantize::Factory::default();
  std::thread::scope(|scope| {
    let workers: Vec<_> = (0..2)
      .map(|_| {
        scope.spawn(|| {
          for _ in 0..16 {
            check(&mut Quantizer::new(&factory).unwrap(), 256, 128, 1);
          }
        })
      })
      .collect();
    for worker in workers {
      worker.join().unwrap();
    }
  });
}

#[test]
fn test_convert_first_gpu_write_is_visible_in_never_cpu_mapped_oversized_output()
 {
  let source = Buffer::allocate(512 * 128 * 3 / 2).unwrap();
  {
    let mut bytes = source.write().unwrap();
    for row in bytes.chunks_mut(512) {
      row.fill(0);
      for group in row[..320].chunks_exact_mut(5) {
        group.copy_from_slice(&[0, 2, 8, 32, 128]);
      }
    }
  }
  for _ in 0..16 {
    let mut q =
      Quantizer::new(&crate::quantize::Factory::default()).unwrap();
    let destination = Buffer::allocate(512 * 256 * 3).unwrap();
    q.convert(
      Depth::Ten,
      256,
      128,
      Plane::new(source.as_fd(), 512, 128),
      Plane::new(destination.as_fd(), 256, 128),
    )
    .unwrap();
    let pixels = destination.read().unwrap();
    assert!(pixels[..256 * 128 * 3 / 2].iter().all(|&v| v == 128));
  }
}

// Tests synchronously observe completion through the public submission
// API.
trait Complete {
  fn convert(
    &mut self,
    depth: Depth,
    width: u32,
    height: u32,
    source: Plane<'_>,
    destination: Plane<'_>,
  ) -> std::io::Result<()>;
}
impl Complete for Quantizer {
  fn convert(
    &mut self,
    depth: Depth,
    width: u32,
    height: u32,
    source: Plane<'_>,
    destination: Plane<'_>,
  ) -> std::io::Result<()> {
    self.submit(depth, width, height, source, destination)?;
    while !self.finish()? {
      std::thread::sleep(std::time::Duration::from_micros(100));
    }
    Ok(())
  }
}

#[test]
fn test_submit_drop_waits_for_started_gpu_work_before_releasing_imports()
 {
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  source.write().unwrap().fill(37);
  target.write().unwrap().fill(0);
  let mut quantizer =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  quantizer
    .submit(
      Depth::Eight,
      32,
      32,
      Plane::new(source.as_fd(), 32, 32),
      Plane::new(target.as_fd(), 32, 32),
    )
    .unwrap();
  // Both CPU producers have completed. finish advances dependency
  // acquisition and submits GPU work; its return may be either
  // pending or complete.
  let _complete = quantizer.finish().unwrap();
  drop(quantizer);
  assert!(
    target.read().unwrap()[..1536].iter().all(|value| *value == 37)
  );
}

#[test]
fn test_submit_cpu_owned_source_is_rejected_and_releases_target() {
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let mut quantizer =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  let cpu = source.write().unwrap();
  let result = quantizer.submit(
    Depth::Eight,
    32,
    32,
    Plane::from_buffer(&source, 32, 32),
    Plane::from_buffer(&target, 32, 32),
  );
  assert_eq!(
    result.unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  target.write().unwrap().finish().unwrap();
  cpu.finish().unwrap();
  quantizer
    .convert(
      Depth::Eight,
      32,
      32,
      Plane::from_buffer(&source, 32, 32),
      Plane::from_buffer(&target, 32, 32),
    )
    .unwrap();
}

#[test]
fn test_submit_cpu_owned_target_alias_is_rejected_and_releases_source()
{
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let alias = Buffer::from_owned(
    target.as_fd().try_clone_to_owned().unwrap(),
    4096,
  )
  .unwrap();
  let mut quantizer =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  let cpu = alias.read().unwrap();
  let result = quantizer.submit(
    Depth::Eight,
    32,
    32,
    Plane::from_buffer(&source, 32, 32),
    Plane::from_buffer(&target, 32, 32),
  );
  assert_eq!(
    result.unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  source.write().unwrap().finish().unwrap();
  cpu.finish().unwrap();
  quantizer
    .convert(
      Depth::Eight,
      32,
      32,
      Plane::from_buffer(&source, 32, 32),
      Plane::from_buffer(&target, 32, 32),
    )
    .unwrap();
}

#[test]
fn test_submit_reserves_both_images_until_completion_or_cancellation() {
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let mut quantizer =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  quantizer
    .submit(
      Depth::Eight,
      32,
      32,
      Plane::from_buffer(&source, 32, 32),
      Plane::from_buffer(&target, 32, 32),
    )
    .unwrap();
  for buffer in [&source, &target] {
    assert_eq!(
      buffer.read().err().unwrap().kind(),
      std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
      buffer.write().err().unwrap().kind(),
      std::io::ErrorKind::WouldBlock
    );
  }
  drop(quantizer);
  source.write().unwrap().finish().unwrap();
  target.write().unwrap().finish().unwrap();
}

#[test]
fn test_submit_independent_contexts_cannot_overlap_the_same_storage() {
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  let other_target = Buffer::allocate(4096).unwrap();
  source.write().unwrap().fill(29);
  let factory = crate::quantize::Factory::default();
  let mut first = Quantizer::new(&factory).unwrap();
  let mut second = Quantizer::new(&factory).unwrap();
  first
    .submit(
      Depth::Eight,
      32,
      32,
      Plane::from_buffer(&source, 32, 32),
      Plane::from_buffer(&target, 32, 32),
    )
    .unwrap();
  assert_eq!(
    second
      .submit(
        Depth::Eight,
        32,
        32,
        Plane::from_buffer(&source, 32, 32),
        Plane::from_buffer(&other_target, 32, 32),
      )
      .unwrap_err()
      .kind(),
    std::io::ErrorKind::WouldBlock
  );
  other_target.write().unwrap().finish().unwrap();
  while !first.finish().unwrap() {
    std::thread::sleep(std::time::Duration::from_micros(100));
  }
  second
    .convert(
      Depth::Eight,
      32,
      32,
      Plane::from_buffer(&source, 32, 32),
      Plane::from_buffer(&other_target, 32, 32),
    )
    .unwrap();
  assert!(
    other_target.read().unwrap()[..1536].iter().all(|v| *v == 29)
  );
}

#[test]
fn test_submit_regular_file_images_reject_and_allow_retry() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let path = std::env::temp_dir().join(format!(
    "v4l2-rkmpp-quantize-regular-{}-{nonce}",
    std::process::id()
  ));
  let regular = std::fs::File::create_new(&path).unwrap();
  std::fs::remove_file(path).unwrap();
  regular.set_len(4096).unwrap();
  let source = Buffer::allocate(4096).unwrap();
  let target = Buffer::allocate(4096).unwrap();
  source.write().unwrap().fill(29);
  let a = Plane::from_buffer(&source, 32, 32);
  let b = Plane::from_buffer(&target, 32, 32);
  let invalid = Plane::new(regular.as_fd(), 32, 32);
  let mut quantizer =
    Quantizer::new(&crate::quantize::Factory::default()).unwrap();
  for (input, output) in [(invalid, b), (a, invalid)] {
    target.write().unwrap().fill(0xa5);
    assert!(
      quantizer.submit(Depth::Eight, 32, 32, input, output).is_err()
    );
    assert_eq!(source.read().unwrap()[0], 29);
    assert!(target.read().unwrap().iter().all(|byte| *byte == 0xa5));
    quantizer.convert(Depth::Eight, 32, 32, a, b).unwrap();
    assert!(
      target.read().unwrap()[..1536].iter().all(|byte| *byte == 29)
    );
  }
}
