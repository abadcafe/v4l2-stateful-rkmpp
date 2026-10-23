//! module: syntax::h264_tests; responsibility: define normalized H.264
//! SPS. operations: test entrypoints; entities: none; tests: geometry,
//! buffering inference, level constraints and truncated syntax via
//! parse.
#[test]
fn test_parse_rejects_truncated_sps() {
  for length in 0..20 {
    assert!(super::h264::parse(&vec![0; length]).is_err());
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
  assert!(super::h264::parse(&bytes).is_err());
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
  maximum: u32,
  buffering: Option<(u32, u32)>,
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
  bits.push_str(&ue(maximum));
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
  if let Some((reorder, capacity)) = buffering {
    // VUI: no optional metadata/HRD, explicit bitstream restriction.
    bits.push_str("100000000101111");
    bits.push_str(&ue(reorder));
    bits.push_str(&ue(capacity));
    bits.push('1');
  } else {
    bits.push_str("01"); // no VUI, RBSP stop
  }
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
  use super::h264::{Poc as Parameters, parse};
  for profile in [66, 100] {
    for poc in 0..3 {
      for frame in [false, true] {
        for gaps in [false, true] {
          let s =
            parse(&sequence(profile, 0, poc, frame, gaps, 4, None))
              .unwrap();
          assert_eq!(
            (s.geometry.id, s.geometry.left, s.geometry.top),
            (7, 2, if frame { 6 } else { 12 })
          );
          assert_eq!(
            (s.frame_bits, s.maximum, s.gaps_allowed),
            (8, 4, gaps)
          );
          match (poc, s.poc) {
            (0, Parameters::Lsb(10))
            | (1, Parameters::Delta { .. })
            | (2, Parameters::FrameNumber) => {}
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
fn test_parse_infers_zero_reference_buffering_for_constrained_intra_profile()
 {
  let s =
    super::h264::parse(&sequence(100, 16, 2, true, false, 0, None))
      .unwrap();
  assert_eq!(s.geometry.limits.reorder(), 0);
  assert_eq!(s.geometry.limits.capacity(), 1);
  let reference =
    super::h264::parse(&sequence(100, 16, 2, true, false, 1, None))
      .unwrap();
  assert_eq!(reference.maximum, 1);
  assert_eq!(reference.geometry.limits.capacity(), 16);
  let baseline =
    super::h264::parse(&sequence(66, 16, 2, true, false, 4, None))
      .unwrap();
  assert_eq!(baseline.geometry.limits.reorder(), 16);
  assert_eq!(baseline.geometry.limits.capacity(), 16);
}

#[test]
fn test_parse_signaled_buffering_overrides_profile_inference() {
  let s = super::h264::parse(&sequence(
    100,
    16,
    2,
    true,
    false,
    4,
    Some((0, 4)),
  ))
  .unwrap();
  assert_eq!(s.geometry.limits.reorder(), 0);
  assert_eq!(s.geometry.limits.capacity(), 4);
}

#[test]
fn test_parse_rejects_buffering_below_reference_count_including_zero() {
  for (references, capacity) in [(4, 2), (1, 0)] {
    assert!(
      super::h264::parse(&sequence(
        66,
        0,
        2,
        true,
        false,
        references,
        Some((0, capacity)),
      ))
      .is_err()
    );
  }
  let no_references = super::h264::parse(&sequence(
    66,
    0,
    2,
    true,
    false,
    0,
    Some((0, 0)),
  ))
  .unwrap();
  assert_eq!(no_references.geometry.limits.capacity(), 1);
}

#[test]
fn test_parse_level_hint_does_not_reject_bounded_reference_and_vui_requirements()
 {
  // Level 1.1: MaxDpbMbs=900, 256x128=128 macroblocks => 7 frames.
  for (references, buffering, valid) in [
    (7, None, true),
    (8, None, true),
    (0, Some((0, 8)), true),
    (7, Some((6, 7)), true),
  ] {
    let mut bytes =
      sequence(66, 0, 2, true, false, references, buffering);
    bytes[2] = 11;
    assert_eq!(super::h264::parse(&bytes).is_ok(), valid);
  }
}

#[test]
fn test_parse_unknown_level_keeps_safe_buffering_inference() {
  for buffering in [None, Some((2, 4))] {
    let mut bytes = sequence(66, 0, 2, true, false, 4, buffering);
    bytes[2] = 255;
    let sequence = super::h264::parse(&bytes).unwrap();
    assert_eq!(sequence.maximum, 4);
    assert_eq!(
      sequence.geometry.limits.capacity(),
      if buffering.is_some() { 4 } else { 16 }
    );
  }
}
