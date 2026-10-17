//! Public H264 SPS geometry contract.
#[test]
fn test_parse_rejects_truncated_sps() {
  for length in 0..20 {
    assert!(super::sequence::parse(&vec![0; length]).is_err());
  }
}
#[test]
fn test_sps_rejects_overflowing_interlaced_crop() {
  // Baseline profile, POC type 2, interlaced 4:2:0, maximal top crop.
  let mut bits = String::from("010000100000000000011110");
  bits.push('1'); // SPS id
  bits.push('1'); // log2_max_frame_num_minus4
  bits.push_str("011"); // pic_order_cnt_type = 2
  bits.push('1'); // max_num_ref_frames
  bits.push('0'); // gaps
  bits.push_str("11"); // width and height
  bits.push_str("0011"); // interlaced, no MBAFF, direct inference, crop
  bits.push_str("11"); // left and right
  bits.push_str(&"0".repeat(30));
  bits.push_str(&"1".repeat(31)); // top = 2^31 - 2
  bits.push('1'); // bottom
  while !bits.len().is_multiple_of(8) {
    bits.push('0');
  }
  let bytes: Vec<u8> = bits
    .as_bytes()
    .chunks(8)
    .map(|chunk| {
      chunk.iter().fold(0, |acc, bit| (acc << 1) | (bit - b'0'))
    })
    .collect();
  assert!(super::sequence::parse(&bytes).is_err());
}

fn ue(value: u32) -> String {
  let code = format!("{:b}", u64::from(value) + 1);
  format!("{}{}", "0".repeat(code.len() - 1), code)
}
fn sequence(
  profile: u8,
  constraints: u8,
  poc: u32,
  frame: bool,
  gaps: bool,
) -> Vec<u8> {
  let mut bits = format!("{profile:08b}{constraints:08b}{:08b}", 30);
  bits.push_str(&ue(7)); // parameter-set identity
  if profile == 100 {
    bits.push_str(&ue(1)); // 4:2:0
    bits.push_str("1100"); // 8-bit, no bypass or scaling matrix
  }
  bits.push_str(&ue(4)); // 8-bit frame_num
  bits.push_str(&ue(poc));
  match poc {
    0 => bits.push_str(&ue(6)), // 10-bit POC LSB
    1 => bits.push_str("1111"), /* deltas always zero, offsets and */
    // cycle = 0
    _ => {}
  }
  bits.push_str(&ue(4));
  bits.push(if gaps { '1' } else { '0' });
  bits.push_str(&ue(15));
  bits.push_str(&ue(7));
  bits.push(if frame { '1' } else { '0' });
  if !frame {
    bits.push('0');
  }
  bits.push_str("11"); // direct inference and crop
  for crop in [1, 2, 3, 4] {
    bits.push_str(&ue(crop));
  }
  bits.push_str("01"); // no VUI, RBSP stop
  while !bits.len().is_multiple_of(8) {
    bits.push('0');
  }
  bits
    .as_bytes()
    .chunks(8)
    .map(|chunk| {
      chunk.iter().fold(0, |acc, bit| (acc << 1) | (bit - b'0'))
    })
    .collect()
}
#[test]
fn test_parse_unifies_geometry_reference_and_order_parameters() {
  use super::sequence::{PictureOrder, parse};
  for profile in [66, 100] {
    for poc in 0..3 {
      for frame in [false, true] {
        for gaps in [false, true] {
          let s =
            parse(&sequence(profile, 0, poc, frame, gaps)).unwrap();
          assert_eq!(
            (s.geometry.id, s.geometry.left, s.geometry.top),
            (7, 2, if frame { 6 } else { 12 })
          );
          assert_eq!(
            (s.frame_bits, s.maximum, s.gaps_allowed),
            (8, 4, gaps)
          );
          assert_eq!(
            (s.frame_only, s.separate, s.chroma),
            (frame, false, 1)
          );
          match (poc, s.order) {
            (0, PictureOrder::Lsb(10))
            | (1, PictureOrder::Delta { always_zero: true })
            | (2, PictureOrder::FrameNumber) => {}
            _ => panic!("wrong parsed POC mode"),
          }
          assert_eq!(s.geometry.limits.capacity(), 16);
          assert_eq!(s.geometry.limits.reorder(), 16);
        }
      }
    }
  }
}
#[test]
fn test_parse_zero_reorder_constraint_preserves_reference_capacity() {
  let s =
    super::sequence::parse(&sequence(100, 16, 2, true, true)).unwrap();
  assert_eq!(s.geometry.limits.reorder(), 0);
  assert_eq!(s.geometry.limits.capacity(), 16);
}
