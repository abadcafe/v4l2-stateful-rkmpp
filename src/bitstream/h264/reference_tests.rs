//! module: bitstream::reference_tests; responsibility: reference
//! identity contract.
use super::reference::{Marking, Memory, Operation, Picture};
#[derive(Default)]
struct Input {
  frame: u32,
  modulus: u32,
  field: Option<bool>,
  reference: bool,
  idr: bool,
  long_idr: bool,
  maximum: u32,
  gaps_allowed: bool,
  adaptive: bool,
  operations: Vec<Operation>,
}
impl From<Input> for Picture {
  fn from(p: Input) -> Self {
    Self {
      frame: p.frame,
      modulus: p.modulus,
      field: p.field,
      maximum: p.maximum,
      gaps_allowed: p.gaps_allowed,
      marking: if p.idr {
        Marking::Idr { long: p.long_idr }
      } else if !p.reference {
        Marking::Disposable
      } else if p.adaptive {
        Marking::Adaptive(p.operations)
      } else {
        Marking::Sliding
      },
    }
  }
}
fn picture(frame: u32) -> Input {
  Input {
    frame,
    modulus: 16,
    field: None,
    reference: true,
    idr: false,
    long_idr: false,
    maximum: 2,
    gaps_allowed: true,
    adaptive: false,
    operations: vec![],
  }
}
#[test]
fn test_sliding_window_retains_only_newest_short_references() {
  let mut memory = Memory::default();
  let a = memory.apply(picture(0).into()).unwrap();
  let b = memory.apply(picture(1).into()).unwrap();
  let c = memory.apply(picture(2).into()).unwrap();
  assert_eq!(a.references, vec![a.id]);
  assert_eq!(b.references, vec![a.id, b.id]);
  assert_eq!(c.references, vec![b.id, c.id]);
}
#[test]
fn test_mmco_short_long_and_reset() {
  let mut memory = Memory::default();
  let a = memory.apply(picture(0).into()).unwrap();
  let mut p = picture(1);
  p.adaptive = true;
  p.operations = vec![Operation::LongFromShort(1, 3)];
  let b = memory.apply(p.into()).unwrap();
  assert_eq!(b.references, vec![a.id, b.id]);
  let mut p = picture(2);
  p.adaptive = true;
  p.operations =
    vec![Operation::ForgetLong(3), Operation::ForgetShort(1)];
  let c = memory.apply(p.into()).unwrap();
  assert_eq!(c.references, vec![c.id]);
  let mut p = picture(3);
  p.adaptive = true;
  p.operations = vec![Operation::Reset];
  let d = memory.apply(p.into()).unwrap();
  assert_eq!(d.references, vec![d.id]);
}
#[test]
fn test_fields_share_identity_and_disposable_pictures_do_not_become_references()
 {
  let mut memory = Memory::default();
  let mut p = picture(0);
  p.field = Some(false);
  let top = memory.apply(p.into()).unwrap();
  let mut p = picture(0);
  p.field = Some(true);
  let bottom = memory.apply(p.into()).unwrap();
  assert_eq!(top.id, bottom.id);
  assert_eq!(bottom.references, vec![top.id]);
  let mut p = picture(1);
  p.reference = false;
  let next = memory.apply(p.into()).unwrap();
  assert_eq!(next.references, vec![top.id]);
}
#[test]
fn test_frame_wrap_and_long_term_limit() {
  let mut memory = Memory::default();
  let a = memory.apply(picture(15).into()).unwrap();
  let mut p = picture(0);
  p.adaptive = true;
  p.operations =
    vec![Operation::LongFromShort(1, 2), Operation::CurrentLong(1)];
  let b = memory.apply(p.into()).unwrap();
  assert_eq!(b.references, vec![a.id, b.id]);
  let mut p = picture(1);
  p.adaptive = true;
  p.operations = vec![Operation::LongLimit(2)];
  let c = memory.apply(p.into()).unwrap();
  assert_eq!(c.references, vec![b.id, c.id]);
}

#[test]
fn test_idr_clears_prior_references_and_zero_capacity_retains_none() {
  let mut memory = Memory::default();
  memory.apply(picture(4).into()).unwrap();
  let mut idr = picture(0);
  idr.idr = true;
  idr.long_idr = true;
  let state = memory.apply(idr.into()).unwrap();
  assert_eq!(state.references, vec![state.id]);
  let mut next = picture(1);
  next.adaptive = true;
  next.operations = vec![Operation::ForgetLong(0)];
  let state = memory.apply(next.into()).unwrap();
  assert_eq!(state.references, vec![state.id]);
  let mut intra = picture(0);
  intra.idr = true;
  intra.maximum = 0;
  assert!(memory.apply(intra.into()).unwrap().references.is_empty());
}

#[test]
fn test_apply_pairing_requires_same_reference_class_and_no_new_boundary()
 {
  for first_reference in [false, true] {
    for second_reference in [false, true] {
      for boundary in 0..3 {
        if boundary != 0 && !second_reference {
          continue;
        }
        let mut memory = Memory::default();
        let mut first = picture(0);
        first.field = Some(false);
        first.reference = first_reference;
        let a = memory.apply(first.into()).unwrap();
        let mut second = picture(0);
        second.field = Some(true);
        second.reference = second_reference;
        second.idr = boundary == 1;
        if boundary == 2 {
          second.adaptive = true;
          second.operations.push(Operation::Reset);
        }
        let b = memory.apply(second.into()).unwrap();
        assert_eq!(
          a.id == b.id,
          first_reference == second_reference && boundary == 0
        );
      }
    }
  }
}

#[test]
fn test_apply_long_index_replacement_retires_other_pair_completely() {
  for bottom in [false, true] {
    for promote in [false, true] {
      let mut memory = Memory::default();
      let mut first = picture(0);
      first.field = Some(bottom);
      first.idr = true;
      first.long_idr = true;
      first.maximum = 4;
      let old = memory.apply(first.into()).unwrap().id;
      let mut second = picture(0);
      second.field = Some(!bottom);
      second.adaptive = true;
      second.maximum = 4;
      second.operations.push(Operation::CurrentLong(0));
      assert_eq!(
        memory.apply(second.into()).unwrap().references,
        vec![old]
      );
      let mut next = picture(1);
      next.maximum = 4;
      next.field = Some(bottom);
      next.adaptive = true;
      if !promote {
        next.operations.push(Operation::CurrentLong(0));
      }
      let mut result = memory.apply(next.into()).unwrap();
      if promote {
        let mut command = picture(2);
        command.maximum = 4;
        command.field = Some(bottom);
        command.adaptive = true;
        command.operations.push(Operation::LongFromShort(2, 0));
        result = memory.apply(command.into()).unwrap();
      }
      assert!(!result.references.contains(&old));
    }
  }
}

#[test]
fn test_apply_gaps_generate_logical_references_and_evict_old_pictures()
{
  for (start, end) in [(0, 3), (14, 1)] {
    for reference in [false, true] {
      let mut memory = Memory::default();
      let old = memory.apply(picture(start).into()).unwrap().id;
      let mut next = picture(end);
      next.reference = reference;
      let result = memory.apply(next.into()).unwrap();
      assert!(!result.references.contains(&old));
      assert_eq!(result.references.len(), 2);
      assert_eq!(result.references.contains(&result.id), reference);
    }
  }
}

#[test]
fn test_apply_forbidden_gap_reports_loss_without_mutating_reference_state()
 {
  let mut memory = Memory::default();
  let old = memory.apply(picture(0).into()).unwrap().id;
  let mut invalid = picture(3);
  invalid.gaps_allowed = false;
  assert!(memory.apply(invalid.into()).is_err());
  let state = memory.apply(picture(1).into()).unwrap();
  assert_eq!(state.references, vec![old, state.id]);
}

#[test]
fn test_apply_reset_restarts_gap_tracking_and_field_identity() {
  let mut memory = Memory::default();
  memory.apply(picture(8).into()).unwrap();
  let mut reset = picture(9);
  reset.adaptive = true;
  reset.operations.push(Operation::Reset);
  let old = memory.apply(reset.into()).unwrap().id;
  let mut next = picture(1);
  next.gaps_allowed = false;
  let state = memory.apply(next.into()).unwrap();
  assert_eq!(state.references, vec![old, state.id]);
}

#[test]
fn test_apply_rejects_out_of_domain_marking_before_changing_references()
{
  for operation in [
    Operation::CurrentLong(u32::MAX),
    Operation::LongFromShort(1, 16),
    Operation::ForgetLong(32),
    Operation::ForgetShort(0),
    Operation::LongLimit(17),
  ] {
    let mut memory = Memory::default();
    let old = memory.apply(picture(0).into()).unwrap().id;
    let mut input = picture(1);
    input.adaptive = true;
    input.operations.push(operation);
    assert!(memory.apply(input.into()).is_err());
    let next = memory.apply(picture(1).into()).unwrap();
    assert_eq!(next.references, vec![old, next.id]);
  }
}

#[test]
fn test_apply_mmco5_first_field_normalizes_pair_identity_and_occupancy()
{
  for bottom in [false, true] {
    for maximum in [1, 4] {
      for number in [1, 5, 15] {
        let mut memory = Memory::default();
        for frame in 0..number {
          let mut input = picture(frame);
          input.maximum = maximum;
          input.gaps_allowed = false;
          input.idr = frame == 0;
          memory.apply(input.into()).unwrap();
        }
        let mut first = picture(number);
        first.field = Some(bottom);
        first.maximum = maximum;
        first.gaps_allowed = false;
        first.adaptive = true;
        first.operations.push(Operation::Reset);
        let first = memory.apply(first.into()).unwrap();
        let mut second = picture(0);
        second.field = Some(!bottom);
        second.maximum = maximum;
        second.gaps_allowed = false;
        let second = memory.apply(second.into()).unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.continuation);
        assert_eq!(second.references, vec![first.id]);
        let mut next = picture(1);
        next.maximum = maximum;
        next.gaps_allowed = false;
        let next = memory.apply(next.into()).unwrap();
        assert!(!next.continuation);
        assert_eq!(next.references.contains(&first.id), maximum > 1);
      }
    }
  }
}
