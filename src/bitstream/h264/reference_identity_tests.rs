//! module: bitstream::h264::reference_identity_tests; responsibility:
//! define gap collisions through the reference-memory API.
//! operations: Memory apply/reset; entities: none; tests: live short
//! references, generated references and retained long references.
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
