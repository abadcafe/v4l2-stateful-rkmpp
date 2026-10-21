//! module: hevc_rps_tests; responsibility: reference identity and
//! retention.
use codec_parser::Limits;

use super::hevc_rps::{Memory, Target};
#[test]
fn test_reset_forgets_references_but_preserves_identity() {
  let mut memory = Memory::default();
  let old = memory.apply(0, &[], true, Limits::default()).unwrap();
  memory.reset();
  let next = memory
    .apply(2, &[Target::Short(0)], false, Limits::default())
    .unwrap();
  assert!(next.0 > old.0);
  assert_eq!(next.1, [next.0]);
}
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
