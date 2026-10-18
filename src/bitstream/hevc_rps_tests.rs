//! module: hevc_rps_tests; responsibility: reference identity and
//! retention.
use super::hevc_rps::{Memory, Target};
use crate::picture::Limits;
#[test]
fn test_apply_keeps_following_references_and_promotes_long_term() {
  let mut m = Memory::default();
  let a = m.apply(0, &[], true, Limits::default()).unwrap();
  let b =
    m.apply(2, &[Target::Short(0)], false, Limits::default()).unwrap();
  assert_eq!(b.1, vec![a.0, b.0]);
  let c = m
    .apply(
      4,
      &[Target::Long { lsb: 0, mask: 15, full: None }],
      false,
      Limits::default(),
    )
    .unwrap();
  assert_eq!(c.1, vec![a.0, c.0]);
  let d =
    m.apply(6, &[Target::Short(0)], false, Limits::default()).unwrap();
  assert_eq!(d.1, vec![d.0]); // a is long-term and cannot match a short-term target.
}
#[test]
fn test_apply_new_sequence_never_reuses_picture_identity() {
  let mut m = Memory::default();
  let a = m.apply(0, &[], true, Limits::default()).unwrap();
  let b = m.apply(0, &[], true, Limits::default()).unwrap();
  assert_ne!(a.0, b.0);
  assert_eq!(b.1, vec![b.0]);
}

fn bits(parts: &[&str]) -> Vec<u8> {
  let mut data = parts.concat();
  while !data.len().is_multiple_of(8) {
    data.push('0');
  }
  let bytes: Vec<u8> = data
    .as_bytes()
    .chunks(8)
    .map(|c| c.iter().fold(0, |v, b| v * 2 + b - b'0'))
    .collect();
  bytes
}
#[test]
fn test_parse_explicit_set_preserves_unused_following_pictures() {
  let bytes = bits(&["011", "010", "1", "0", "010", "1", "010", "0"]);
  let mut b = super::bits::Bits::from_rbsp(&bytes);
  assert_eq!(
    super::hevc_rps::parse(&mut b, &[], false).unwrap(),
    vec![-1, -3, 2]
  );
}
#[test]
fn test_parse_prediction_sorts_negative_and_positive_deltas() {
  let bytes = bits(&["1", "0", "1", "1111"]);
  let mut b = super::bits::Bits::from_rbsp(&bytes);
  assert_eq!(
    super::hevc_rps::parse(&mut b, &[vec![-1, -3, 2]], false).unwrap(),
    vec![-2, 1, 3]
  );
}
#[test]
fn test_parse_inline_prediction_uses_declared_prior_index() {
  let bytes = bits(&["1", "010", "1", "1", "11"]);
  let mut b = super::bits::Bits::from_rbsp(&bytes);
  assert_eq!(
    super::hevc_rps::parse(&mut b, &[vec![-2], vec![-7]], true)
      .unwrap(),
    vec![-1, -3]
  );
}
#[test]
fn test_parse_rejects_truncated_and_invalid_prediction() {
  assert!(
    super::hevc_rps::parse(
      &mut super::bits::Bits::from_rbsp(&[]),
      &[],
      false
    )
    .is_err()
  );
  let bytes = bits(&["1", "010"]);
  let mut b = super::bits::Bits::from_rbsp(&bytes);
  assert!(super::hevc_rps::parse(&mut b, &[vec![-1]], true).is_err());
}
#[test]
fn test_apply_full_long_term_poc_distinguishes_equal_lsb() {
  let mut m = Memory::default();
  let a = m.apply(0, &[], true, Limits::default()).unwrap();
  let b =
    m.apply(16, &[Target::Short(0)], false, Limits::default()).unwrap();
  let c = m
    .apply(
      18,
      &[Target::Long { lsb: 0, mask: 15, full: Some(16) }],
      false,
      Limits::default(),
    )
    .unwrap();
  assert_eq!(c.1, vec![b.0, c.0]);
  assert!(!c.1.contains(&a.0));
}

#[test]
fn test_apply_rejects_references_exceeding_the_operating_point_capacity()
 {
  let mut memory = Memory::default();
  let limits = Limits::new(1, 0, None).unwrap();
  memory.apply(0, &[], true, limits).unwrap();
  assert!(memory.apply(2, &[Target::Short(0)], false, limits).is_err());
  let reset = memory.apply(0, &[], true, limits).unwrap();
  assert_eq!(reset.1, [reset.0]);
}
