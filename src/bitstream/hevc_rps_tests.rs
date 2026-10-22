//! module: hevc_rps_tests; responsibility: reference-set admission.
//! operations: References new, Memory reset/apply; entities: none;
//! tests: identity, selection, required/following usage and syntax
//! bounds.
use std::io;

use codec_parser::Limits;
use codec_parser::header::{Long, Short};

use super::hevc_rps::{Memory, Picture, References};

fn picture(poc: i32) -> Picture {
  Picture { poc, kind: 1, temporal_id: 0, temporal_nested: false }
}

fn apply(
  memory: &mut Memory,
  poc: i32,
  short: &[Short],
  long: &[Long],
  reset: bool,
) -> io::Result<(u64, Vec<u64>)> {
  memory.apply(
    References::new(picture(poc), 4, short, long)?,
    reset,
    Limits::default(),
  )
}

#[test]
fn test_reset_forgets_references_but_preserves_identity() {
  let mut memory = Memory::default();
  let old = apply(&mut memory, 0, &[], &[], true).unwrap();
  memory.reset();
  let next = apply(
    &mut memory,
    2,
    &[Short { delta: -2, used: false }],
    &[],
    false,
  )
  .unwrap();
  assert!(next.0 > old.0);
  assert_eq!(next.1, [next.0]);
}

#[test]
fn test_apply_long_term_distance_rejects_the_limit_before_mutation() {
  for used in [false, true] {
    let mut memory = Memory::default();
    let old = apply(&mut memory, 0, &[], &[], true).unwrap().0;
    assert!(
      apply(
        &mut memory,
        1 << 24,
        &[],
        &[Long { lsb: 0, cycle: Some(1 << 20), used }],
        false
      )
      .is_err()
    );
    let next = apply(
      &mut memory,
      (1 << 24) - 1,
      &[],
      &[Long { lsb: 0, cycle: Some((1 << 20) - 1), used }],
      false,
    )
    .unwrap();
    assert_eq!(next.1, [old, next.0]);
  }
}

#[test]
fn test_apply_keeps_following_references_and_promotes_long_term() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap();
  let b = apply(
    &mut memory,
    2,
    &[Short { delta: -2, used: false }],
    &[],
    false,
  )
  .unwrap();
  assert_eq!(b.1, [a.0, b.0]);
  let c = apply(
    &mut memory,
    4,
    &[],
    &[Long { lsb: 0, cycle: None, used: false }],
    false,
  )
  .unwrap();
  assert_eq!(c.1, [a.0, c.0]);
  let d = apply(
    &mut memory,
    6,
    &[Short { delta: -6, used: false }],
    &[],
    false,
  )
  .unwrap();
  assert_eq!(d.1, [d.0]); // An old long-term picture cannot match a short-term target.
}

#[test]
fn test_apply_new_sequence_never_reuses_picture_identity() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap();
  let b = apply(&mut memory, 0, &[], &[], true).unwrap();
  assert_ne!(a.0, b.0);
  assert_eq!(b.1, [b.0]);
}

#[test]
fn test_apply_full_long_term_poc_distinguishes_equal_lsb() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap();
  let b = apply(
    &mut memory,
    16,
    &[Short { delta: -16, used: true }],
    &[],
    false,
  )
  .unwrap();
  let c = apply(
    &mut memory,
    18,
    &[],
    &[Long { lsb: 0, cycle: Some(0), used: true }],
    false,
  )
  .unwrap();
  assert_eq!(c.1, [b.0, c.0]);
  assert!(!c.1.contains(&a.0));
}

#[test]
fn test_apply_distinct_full_long_pocs_may_share_lsb() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap().0;
  let b = apply(
    &mut memory,
    16,
    &[Short { delta: -16, used: true }],
    &[],
    false,
  )
  .unwrap()
  .0;
  let c = apply(
    &mut memory,
    18,
    &[],
    &[
      Long { lsb: 0, cycle: Some(1), used: true },
      Long { lsb: 0, cycle: Some(0), used: true },
    ],
    false,
  )
  .unwrap();
  assert_eq!(c.1, [a, b, c.0]);
}

#[test]
fn test_apply_long_cycle_preserves_negative_poc_arithmetic() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, -16, &[], &[], true).unwrap().0;
  let b = apply(
    &mut memory,
    -1,
    &[],
    &[Long { lsb: 0, cycle: Some(0), used: true }],
    false,
  )
  .unwrap();
  assert_eq!(b.1, [a, b.0]);
}

#[test]
fn test_apply_reset_cannot_satisfy_a_required_reference_from_prior_sequence()
 {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap().0;
  assert!(
    apply(
      &mut memory,
      2,
      &[Short { delta: -2, used: true }],
      &[],
      true
    )
    .is_err()
  );
  let next = apply(
    &mut memory,
    2,
    &[Short { delta: -2, used: true }],
    &[],
    false,
  )
  .unwrap();
  assert_eq!(next.1, [a, next.0]);
}

#[test]
fn test_apply_rejects_ambiguous_long_lsb_before_promoting_references() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap();
  let b = apply(
    &mut memory,
    16,
    &[Short { delta: -16, used: false }],
    &[],
    false,
  )
  .unwrap();
  assert!(
    apply(
      &mut memory,
      18,
      &[],
      &[Long { lsb: 0, cycle: None, used: false }],
      false
    )
    .is_err()
  );
  let recovered = apply(
    &mut memory,
    18,
    &[
      Short { delta: -18, used: false },
      Short { delta: -2, used: false },
    ],
    &[],
    false,
  )
  .unwrap();
  assert_eq!(recovered.1, [a.0, b.0, recovered.0]);
}

#[test]
fn test_new_rejects_duplicate_and_cross_class_rps_entries() {
  for (short, long) in [
    (
      vec![
        Short { delta: -2, used: true },
        Short { delta: -2, used: false },
      ],
      vec![],
    ),
    (
      vec![],
      vec![
        Long { lsb: 0, cycle: Some(0), used: true },
        Long { lsb: 0, cycle: Some(0), used: false },
      ],
    ),
    (
      vec![Short { delta: -2, used: false }],
      vec![Long { lsb: 0, cycle: Some(0), used: false }],
    ),
    (
      vec![Short { delta: -2, used: false }],
      vec![Long { lsb: 0, cycle: None, used: true }],
    ),
  ] {
    let mut memory = Memory::default();
    let old = apply(&mut memory, 0, &[], &[], true).unwrap().0;
    assert!(apply(&mut memory, 2, &short, &long, false).is_err());
    let next = apply(
      &mut memory,
      2,
      &[Short { delta: -2, used: true }],
      &[],
      false,
    )
    .unwrap();
    assert_eq!(next.1, [old, next.0]);
  }
}

#[test]
fn test_apply_rejects_references_exceeding_the_operating_point_capacity()
 {
  let mut memory = Memory::default();
  let limits = Limits::new(1, 0, None).unwrap();
  memory
    .apply(
      References::new(picture(0), 4, &[], &[]).unwrap(),
      true,
      limits,
    )
    .unwrap();
  assert!(
    memory
      .apply(
        References::new(
          picture(2),
          4,
          &[Short { delta: -2, used: true }],
          &[]
        )
        .unwrap(),
        false,
        limits
      )
      .is_err()
  );
  let reset = memory
    .apply(
      References::new(picture(0), 4, &[], &[]).unwrap(),
      true,
      limits,
    )
    .unwrap();
  assert_eq!(reset.1, [reset.0]);
}

#[test]
fn test_new_rejects_unrepresentable_reference_syntax() {
  for bits in [0, 3, 17, usize::MAX] {
    assert!(References::new(picture(0), bits, &[], &[]).is_err());
  }
  for (poc, delta) in [(0, 0), (i32::MAX, 1), (i32::MIN, -1)] {
    assert!(
      References::new(
        picture(poc),
        4,
        &[Short { delta, used: false }],
        &[]
      )
      .is_err()
    );
  }
  for long in [
    Long { lsb: 16, cycle: None, used: false },
    Long { lsb: 0, cycle: Some(u64::MAX), used: false },
  ] {
    assert!(References::new(picture(0), 4, &[], &[long]).is_err());
  }
  let too_many: Vec<_> = (1..=17)
    .map(|delta| Short { delta: -delta, used: false })
    .collect();
  assert!(References::new(picture(0), 4, &too_many, &[]).is_err());
}

#[test]
fn test_new_rejects_invalid_temporal_picture_metadata() {
  for (kind, temporal_id) in [(32, 0), (1, 7), (2, 0), (4, 0), (19, 1)]
  {
    assert!(
      References::new(
        Picture { poc: 0, kind, temporal_id, temporal_nested: false },
        4,
        &[],
        &[]
      )
      .is_err()
    );
  }
}

#[test]
fn test_apply_required_missing_references_reject_but_following_can_be_absent()
 {
  for used in [false, true] {
    for long_term in [false, true] {
      let mut memory = Memory::default();
      let old = apply(&mut memory, 0, &[], &[], true).unwrap().0;
      let result = if long_term {
        apply(
          &mut memory,
          2,
          &[],
          &[Long { lsb: 1, cycle: None, used }],
          false,
        )
      } else {
        apply(&mut memory, 2, &[Short { delta: -1, used }], &[], false)
      };
      assert_eq!(result.is_err(), used);
      if used {
        let next = apply(
          &mut memory,
          2,
          &[Short { delta: -2, used: true }],
          &[],
          false,
        )
        .unwrap();
        assert_eq!(next.1, [old, next.0]);
      } else {
        let next = result.unwrap();
        assert_eq!(next.1, [next.0]);
      }
    }
  }
}

#[test]
fn test_apply_required_short_reference_cannot_reuse_a_long_term_picture()
 {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap().0;
  let b = apply(
    &mut memory,
    2,
    &[],
    &[Long { lsb: 0, cycle: None, used: true }],
    false,
  )
  .unwrap()
  .0;
  assert!(
    apply(
      &mut memory,
      4,
      &[Short { delta: -4, used: true }],
      &[],
      false
    )
    .is_err()
  );
  let c = apply(
    &mut memory,
    4,
    &[Short { delta: -2, used: true }],
    &[Long { lsb: 0, cycle: None, used: true }],
    false,
  )
  .unwrap();
  assert_eq!(c.1, [a, b, c.0]);
}

#[test]
fn test_apply_borrows_syntax_without_retaining_its_storage() {
  let mut memory = Memory::default();
  let a = apply(&mut memory, 0, &[], &[], true).unwrap().0;
  let mut short = vec![Short { delta: -2, used: true }];
  let b = apply(&mut memory, 2, &short, &[], false).unwrap().0;
  short.clear();
  drop(short);
  let c = apply(
    &mut memory,
    4,
    &[Short { delta: -4, used: true }, Short { delta: -2, used: true }],
    &[],
    false,
  )
  .unwrap();
  assert_eq!(c.1, [a, b, c.0]);
}
