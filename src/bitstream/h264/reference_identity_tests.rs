//! module: bitstream::h264::reference_identity_tests; responsibility:
//! define frame-number admission through the reference-memory API.
//! operations: Memory apply/reset; entities: none; tests: IDR origin,
//! repeated numbers, consecutive pairs and gap/reference collisions.
use super::reference::{Marking, Memory, Picture};

fn picture(
  frame: u32,
  field: Option<bool>,
  marking: Marking<'_>,
) -> Picture<'_> {
  Picture {
    frame,
    modulus: 16,
    field,
    maximum: 4,
    gaps_allowed: true,
    marking,
  }
}

#[test]
fn test_apply_idr_requires_zero_frame_number_before_resetting_references()
 {
  for field in [None, Some(false), Some(true)] {
    for number in [1, 15] {
      let mut memory = Memory::default();
      let first = memory
        .apply(picture(0, None, Marking::Idr { long: false }))
        .unwrap();
      assert!(
        memory
          .apply(picture(number, field, Marking::Idr { long: false }))
          .is_err()
      );
      let next =
        memory.apply(picture(1, None, Marking::Sliding)).unwrap();
      assert_eq!(next.references, [first.id, next.id]);
    }
  }
}

#[test]
fn test_apply_equal_frame_number_requires_opposite_reference_fields() {
  for first in [None, Some(false), Some(true)] {
    for second in [None, Some(false), Some(true)] {
      for reference in [false, true] {
        let mut memory = Memory::default();
        let old = memory
          .apply(picture(0, first, Marking::Idr { long: false }))
          .unwrap();
        let marking = if reference {
          Marking::Sliding
        } else {
          Marking::Disposable
        };
        let result = memory.apply(picture(0, second, marking));
        let paired = reference
          && first.is_some()
          && second.is_some()
          && first != second;
        assert_eq!(result.is_ok(), paired);
        if let Ok(result) = result {
          assert!(result.continuation);
          assert_eq!(result.id, old.id);
          assert_eq!(result.references, [old.id]);
        }
      }
    }
  }
}

#[test]
fn test_apply_repeated_number_cannot_start_a_third_reference_field() {
  for bottom in [false, true] {
    let mut memory = Memory::default();
    let first = memory
      .apply(picture(0, Some(bottom), Marking::Idr { long: false }))
      .unwrap();
    let second = memory
      .apply(picture(0, Some(!bottom), Marking::Sliding))
      .unwrap();
    assert_eq!(second.id, first.id);
    assert!(
      memory.apply(picture(0, Some(bottom), Marking::Sliding)).is_err()
    );
    let next =
      memory.apply(picture(1, None, Marking::Sliding)).unwrap();
    assert_eq!(next.references, [first.id, next.id]);
  }
}

#[test]
fn test_apply_equal_reference_fields_must_be_consecutive_primary_pictures()
 {
  for bottom in [false, true] {
    let mut memory = Memory::default();
    let first = memory
      .apply(picture(0, Some(bottom), Marking::Idr { long: false }))
      .unwrap();
    let middle =
      memory.apply(picture(1, None, Marking::Disposable)).unwrap();
    assert_eq!(middle.references, [first.id]);
    assert!(
      memory
        .apply(picture(0, Some(!bottom), Marking::Sliding))
        .is_err()
    );
    let next =
      memory.apply(picture(1, None, Marking::Sliding)).unwrap();
    assert!(!next.continuation);
    assert_eq!(next.references, [first.id, next.id]);
  }
}

#[test]
fn test_apply_gap_cannot_reuse_a_live_short_term_frame_number() {
  let mut memory = Memory::default();
  let a = memory
    .apply(picture(0, None, Marking::Idr { long: false }))
    .unwrap();
  let b = memory.apply(picture(1, None, Marking::Sliding)).unwrap();
  let c = memory.apply(picture(2, None, Marking::Sliding)).unwrap();
  assert!(memory.apply(picture(1, None, Marking::Sliding)).is_err());
  let d = memory.apply(picture(3, None, Marking::Sliding)).unwrap();
  assert_eq!(d.references, [a.id, b.id, c.id, d.id]);
}

#[test]
fn test_apply_gap_collision_includes_generated_references_and_preserves_previous_number()
 {
  let mut memory = Memory::default();
  memory.apply(picture(0, None, Marking::Idr { long: false })).unwrap();
  let generated =
    memory.apply(picture(3, None, Marking::Disposable)).unwrap();
  assert_eq!(generated.references.len(), 3);
  assert!(memory.apply(picture(1, None, Marking::Sliding)).is_err());
  let mut retry = picture(3, None, Marking::Sliding);
  retry.gaps_allowed = false;
  let next = memory.apply(retry).unwrap();
  let mut expected = generated.references;
  expected.push(next.id);
  assert_eq!(next.references, expected);
}

#[test]
fn test_apply_gap_may_cross_a_live_long_term_frame_number() {
  let mut memory = Memory::default();
  let mut first = picture(0, None, Marking::Idr { long: true });
  first.maximum = 2;
  let first = memory.apply(first).unwrap();
  for frame in [1, 2] {
    let mut input = picture(frame, None, Marking::Sliding);
    input.maximum = 2;
    memory.apply(input).unwrap();
  }
  let mut next = picture(1, None, Marking::Sliding);
  next.maximum = 2;
  let next = memory.apply(next).unwrap();
  assert_eq!(next.references, [first.id, next.id]);
  memory.reset();
  assert!(
    memory
      .apply(picture(0, None, Marking::Idr { long: false }))
      .is_ok()
  );
}
