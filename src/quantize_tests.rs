//! module: quantize_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_convert_pixels_saturation_fractional_mean_padding_and_odd_dimensions
//! test_convert_distinct_buffer_lifetimes_preserve_all_pixels
//! test_convert_4k_frame_preserves_pixels_and_padding
use std::os::fd::AsFd;

use crate::dma::Buffer;
use crate::image::Plane;
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
    q.convert(w, h, source_plane, destination_plane).unwrap();
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
  assert!(q.convert(0, h, source_plane, destination_plane).is_err());
  assert!(
    q.convert(
      w,
      h,
      Plane::new(source.as_fd(), 4, rows),
      destination_plane
    )
    .is_err()
  );
  assert!(
    q.convert(w, h, source_plane, Plane::new(source.as_fd(), ds, rows))
      .is_err()
  );
  q.convert(w, h, source_plane, destination_plane).unwrap();
}

#[test]
fn test_convert_pixels_saturation_fractional_mean_padding_and_odd_dimensions()
 {
  let mut q = Quantizer::new().unwrap();
  check(&mut q, 256, 256, 8);
  check(&mut q, 258, 130, 3);
  check(&mut q, 255, 127, 3);
}

#[test]
fn test_convert_distinct_buffer_lifetimes_preserve_all_pixels() {
  let mut q = Quantizer::new().unwrap();
  for _ in 0..16 {
    check(&mut q, 32, 32, 1);
  }
}

#[test]
fn test_convert_4k_frame_preserves_pixels_and_padding() {
  check(&mut Quantizer::new().unwrap(), 3840, 2160, 2);
}
