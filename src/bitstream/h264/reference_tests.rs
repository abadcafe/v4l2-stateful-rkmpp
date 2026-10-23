//! module: bitstream::reference_tests; responsibility: reference
//! identity contract.
//! operations: test entrypoints; entities: Input; tests:
//! test_reset_clears_reference_and_field_state_but_preserves_identity,
//! test_apply_borrows_canonical_operations_without_retaining_their_storage,
//! test_apply_long_assignment_requires_an_active_limit,
//! test_apply_long_limit_cannot_exceed_current_sps_capacity,
//! test_apply_long_limit_persists_and_reduction_retires_high_indices,
//! test_apply_long_assignment_obeys_command_order_and_inclusive_upper_bound,
//! test_apply_reset_boundaries_clear_long_limit_until_explicitly_reopened,
//! test_apply_long_idr_initializes_only_index_zero,
//! test_apply_reference_commands_require_an_existing_target,
//! test_apply_reference_targets_are_checked_after_each_command,
//! test_apply_gap_placeholders_can_be_forgotten_but_not_promoted,
//! test_apply_frame_commands_require_both_reference_fields,
//! test_apply_frame_commands_accept_complete_frames_and_complementary_pairs,
//! test_apply_large_gap_retains_only_final_synthetic_references,
//! test_apply_gap_cannot_overcommit_a_pool_of_long_term_references,
//! test_sliding_window_retains_only_newest_short_references,
//! test_apply_rejects_adaptive_marking_that_exceeds_reference_capacity,
//! test_apply_rejects_sliding_window_without_an_evictable_reference,
//! test_mmco_short_long_and_reset,
//! test_fields_share_identity_and_disposable_pictures_do_not_become_references,
//! test_frame_wrap_and_long_term_limit,
//! test_idr_clears_prior_references_and_zero_capacity_retains_none,
//! test_apply_pairing_requires_same_reference_class_and_no_new_boundary,
//! test_apply_long_index_replacement_retires_other_pair_completely,
//! test_apply_gaps_generate_logical_references_and_evict_old_pictures,
//! test_apply_forbidden_gap_reports_loss_without_mutating_reference_state,
//! test_apply_reset_restarts_gap_tracking_and_field_identity,
//! test_apply_rejects_out_of_domain_marking_before_changing_references,
//! test_apply_mmco5_first_field_normalizes_pair_identity_and_occupancy,
//! test_apply_rejects_duplicate_and_forbidden_command_combinations,
//! test_apply_later_commands_cannot_retire_current_long_assignment,
//! test_apply_permitted_command_order_and_current_complement_survive.
use codec_parser::header::Operation;

use super::reference::{Marking, Memory, Picture};

#[test]
fn test_reset_clears_reference_and_field_state_but_preserves_identity()
{
  let mut memory = Memory::default();
  let mut first = picture(8);
  first.field = Some(false);
  let old = memory.apply(first.picture()).unwrap();
  memory.reset();
  let mut second = picture(8);
  second.field = Some(true);
  second.gaps_allowed = false;
  let next = memory.apply(second.picture()).unwrap();
  assert!(next.id > old.id);
  assert!(!next.continuation);
  assert_eq!(next.references, [next.id]);
  memory.reset();
  let mut unrelated = picture(3);
  unrelated.gaps_allowed = false;
  assert!(memory.apply(unrelated.picture()).unwrap().id > next.id);
}
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
impl Input {
  fn picture(&self) -> Picture<'_> {
    let p = self;
    Picture {
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
        Marking::Adaptive(&p.operations)
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
fn test_apply_borrows_canonical_operations_without_retaining_their_storage()
 {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.idr = true;
  let old = memory.apply(first.picture()).unwrap().id;
  let mut next = picture(1);
  next.adaptive = true;
  next.operations = vec![
    Operation::LongLimit(1),
    Operation::LongFromShort { difference: 0, index: 0 },
  ];
  let admitted = memory.apply(next.picture()).unwrap();
  assert_eq!(admitted.references, [old, admitted.id]);
  next.operations.clear();
  drop(next);
  let mut third = picture(2);
  third.adaptive = true;
  third.operations = vec![Operation::ForgetLong(0)];
  let third = memory.apply(third.picture()).unwrap();
  assert_eq!(third.references, [admitted.id, third.id]);
}

#[test]
fn test_apply_long_assignment_requires_an_active_limit() {
  for idr in [false, true] {
    for operation in [
      Operation::CurrentLong(0),
      Operation::LongFromShort { difference: 0, index: 0 },
    ] {
      let mut memory = Memory::default();
      let mut first = picture(0);
      first.idr = idr;
      first.maximum = 4;
      let old = memory.apply(first.picture()).unwrap().id;
      let mut invalid = picture(1);
      invalid.maximum = 4;
      invalid.adaptive = true;
      invalid.operations.push(operation);
      assert!(memory.apply(invalid.picture()).is_err());
      let mut retry = picture(1);
      retry.maximum = 4;
      let next = memory.apply(retry.picture()).unwrap();
      assert_eq!(next.references, [old, next.id]);
    }
  }
}

#[test]
fn test_apply_long_limit_cannot_exceed_current_sps_capacity() {
  for maximum in [0, 1, 2, 15] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = maximum;
    first.idr = true;
    memory.apply(first.picture()).unwrap();
    let mut invalid = picture(1);
    invalid.maximum = maximum;
    invalid.adaptive = true;
    invalid.operations.push(Operation::LongLimit(maximum + 1));
    assert!(memory.apply(invalid.picture()).is_err());
    let mut retry = picture(1);
    retry.maximum = maximum;
    retry.adaptive = true;
    retry.operations.push(Operation::LongLimit(maximum));
    if maximum != 0 {
      retry.operations.push(Operation::ForgetShort(0));
    }
    assert!(memory.apply(retry.picture()).is_ok());
  }
}

#[test]
fn test_apply_long_limit_persists_and_reduction_retires_high_indices() {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 4;
  first.idr = true;
  let a = memory.apply(first.picture()).unwrap().id;
  let mut second = picture(1);
  second.maximum = 4;
  second.adaptive = true;
  second.operations = vec![
    Operation::LongLimit(4),
    Operation::LongFromShort { difference: 0, index: 3 },
    Operation::CurrentLong(2),
  ];
  let b = memory.apply(second.picture()).unwrap().id;
  let mut third = picture(2);
  third.maximum = 4;
  let c = memory.apply(third.picture()).unwrap().id;
  let mut fourth = picture(3);
  fourth.maximum = 4;
  fourth.adaptive = true;
  fourth.operations =
    vec![Operation::LongLimit(2), Operation::CurrentLong(1)];
  let d = memory.apply(fourth.picture()).unwrap();
  assert_eq!(d.references, [c, d.id]);
  assert!(!d.references.contains(&a) && !d.references.contains(&b));
  let mut invalid = picture(4);
  invalid.maximum = 4;
  invalid.adaptive = true;
  invalid.operations.push(Operation::CurrentLong(2));
  assert!(memory.apply(invalid.picture()).is_err());
  let mut retry = picture(4);
  retry.maximum = 4;
  retry.adaptive = true;
  retry.operations.push(Operation::CurrentLong(0));
  let e = memory.apply(retry.picture()).unwrap();
  assert_eq!(e.references, [c, d.id, e.id]);
}

#[test]
fn test_apply_long_assignment_obeys_command_order_and_inclusive_upper_bound()
 {
  for maximum in [1, 2, 16] {
    for promote in [false, true] {
      for limit_first in [false, true] {
        let mut memory = Memory::default();
        let mut first = picture(0);
        first.maximum = maximum;
        first.idr = true;
        memory.apply(first.picture()).unwrap();
        let mut next = picture(1);
        next.maximum = maximum;
        next.adaptive = true;
        let assignment = if promote {
          Operation::LongFromShort { difference: 0, index: maximum - 1 }
        } else {
          Operation::CurrentLong(maximum - 1)
        };
        let limit = Operation::LongLimit(maximum);
        next.operations = if limit_first {
          vec![limit, assignment]
        } else {
          vec![assignment, limit]
        };
        // Free the previous frame explicitly when there is just one
        // slot, so capacity cannot mask the index/order result.
        if maximum == 1 {
          next.operations.push(if promote {
            Operation::ForgetLong(0)
          } else {
            Operation::ForgetShort(0)
          });
        }
        assert_eq!(memory.apply(next.picture()).is_ok(), limit_first);
      }
    }
  }
}

#[test]
fn test_apply_reset_boundaries_clear_long_limit_until_explicitly_reopened()
 {
  for boundary in 0..4 {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    first.long_idr = true;
    memory.apply(first.picture()).unwrap();
    let mut second = picture(1);
    second.maximum = 4;
    memory.apply(second.picture()).unwrap();
    let frame = if boundary == 0 {
      memory.reset();
      0
    } else {
      let mut reset = picture(if boundary == 1 { 0 } else { 2 });
      reset.maximum = 4;
      reset.idr = boundary == 1;
      if boundary != 1 {
        reset.adaptive = true;
        reset.operations.push(if boundary == 2 {
          Operation::Reset
        } else {
          Operation::LongLimit(0)
        });
      }
      memory.apply(reset.picture()).unwrap();
      if boundary == 3 { 3 } else { 1 }
    };
    let mut invalid = picture(frame);
    invalid.maximum = 4;
    invalid.adaptive = true;
    invalid.operations.push(Operation::CurrentLong(0));
    assert!(memory.apply(invalid.picture()).is_err());
    let mut retry = picture(frame);
    retry.maximum = 4;
    retry.adaptive = true;
    retry.operations =
      vec![Operation::LongLimit(1), Operation::CurrentLong(0)];
    assert!(memory.apply(retry.picture()).is_ok());
  }
}

#[test]
fn test_apply_long_idr_initializes_only_index_zero() {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 4;
  first.idr = true;
  first.long_idr = true;
  let old = memory.apply(first.picture()).unwrap().id;
  let mut invalid = picture(1);
  invalid.maximum = 4;
  invalid.adaptive = true;
  invalid.operations.push(Operation::CurrentLong(1));
  assert!(memory.apply(invalid.picture()).is_err());
  let mut retry = picture(1);
  retry.maximum = 4;
  retry.adaptive = true;
  retry.operations.push(Operation::CurrentLong(0));
  let next = memory.apply(retry.picture()).unwrap();
  assert_eq!(next.references, [next.id]);
  assert!(!next.references.contains(&old));
}

#[test]
fn test_apply_reference_commands_require_an_existing_target() {
  for operation in [
    Operation::ForgetShort(1),
    Operation::ForgetLong(0),
    Operation::LongFromShort { difference: 1, index: 0 },
  ] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    let old = memory.apply(first.picture()).unwrap().id;
    let mut invalid = picture(1);
    invalid.maximum = 4;
    invalid.adaptive = true;
    invalid.operations = vec![Operation::LongLimit(1), operation];
    assert!(memory.apply(invalid.picture()).is_err());
    let mut retry = picture(1);
    retry.maximum = 4;
    let next = memory.apply(retry.picture()).unwrap();
    assert_eq!(next.references, [old, next.id]);
  }
}

#[test]
fn test_apply_reference_targets_are_checked_after_each_command() {
  for long in [false, true] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    first.long_idr = long;
    memory.apply(first.picture()).unwrap();
    let mut invalid = picture(1);
    invalid.maximum = 4;
    invalid.adaptive = true;
    let operation = if long {
      Operation::ForgetLong(0)
    } else {
      Operation::ForgetShort(0)
    };
    invalid.operations = vec![operation.clone(), operation];
    assert!(memory.apply(invalid.picture()).is_err());
  }
  let mut memory = Memory::default();
  memory.apply(picture(0).picture()).unwrap();
  let mut invalid = picture(1);
  invalid.maximum = 4;
  invalid.adaptive = true;
  invalid.operations = vec![
    Operation::LongLimit(1),
    Operation::LongFromShort { difference: 0, index: 0 },
    Operation::ForgetShort(0),
  ];
  assert!(memory.apply(invalid.picture()).is_err());
}

#[test]
fn test_apply_gap_placeholders_can_be_forgotten_but_not_promoted() {
  for field in [None, Some(false), Some(true)] {
    for promote in [false, true] {
      let mut memory = Memory::default();
      let mut first = picture(0);
      first.maximum = 4;
      first.idr = true;
      let old = memory.apply(first.picture()).unwrap().id;
      let mut next = picture(2);
      next.maximum = 4;
      next.field = field;
      next.adaptive = true;
      let difference = if field.is_some() { 2 } else { 1 };
      next.operations = vec![
        Operation::LongLimit(1),
        if promote {
          Operation::LongFromShort {
            difference: difference - 1,
            index: 0,
          }
        } else {
          Operation::ForgetShort(difference - 1)
        },
      ];
      let result = memory.apply(next.picture());
      if promote {
        assert!(result.is_err());
      } else {
        let result = result.unwrap();
        assert!(result.references.contains(&old));
        assert!(result.references.contains(&result.id));
        assert_eq!(
          result.references.len(),
          if field.is_some() { 3 } else { 2 }
        );
      }
    }
  }
}

#[test]
fn test_apply_frame_commands_require_both_reference_fields() {
  for bottom in [false, true] {
    for frame_mode in [false, true] {
      for command in 0..3 {
        let mut memory = Memory::default();
        let mut first = picture(0);
        first.maximum = 4;
        first.field = Some(bottom);
        first.idr = true;
        first.long_idr = command == 1;
        memory.apply(first.picture()).unwrap();
        let mut next = picture(1);
        next.maximum = 4;
        next.field = if frame_mode { None } else { Some(bottom) };
        next.adaptive = true;
        let difference = if frame_mode { 1 } else { 2 };
        next.operations = vec![
          Operation::LongLimit(1),
          match command {
            0 => Operation::ForgetShort(difference - 1),
            1 => Operation::ForgetLong(u32::from(!frame_mode)),
            2 => Operation::LongFromShort {
              difference: difference - 1,
              index: 0,
            },
            _ => unreachable!(),
          },
        ];
        assert_eq!(memory.apply(next.picture()).is_ok(), !frame_mode);
      }
    }
  }
}

#[test]
fn test_apply_frame_commands_accept_complete_frames_and_complementary_pairs()
 {
  for fields in [false, true] {
    for command in 0..3 {
      let mut memory = Memory::default();
      let mut first = picture(0);
      first.maximum = 4;
      first.field = fields.then_some(false);
      first.idr = true;
      first.long_idr = command == 1;
      let old = memory.apply(first.picture()).unwrap().id;
      if fields {
        let mut second = picture(0);
        second.maximum = 4;
        second.field = Some(true);
        if command == 1 {
          second.adaptive = true;
          second.operations.push(Operation::CurrentLong(0));
        }
        let pair = memory.apply(second.picture()).unwrap();
        assert_eq!(pair.id, old);
        assert!(pair.continuation);
      }
      let mut next = picture(1);
      next.maximum = 4;
      next.adaptive = true;
      next.operations = vec![
        Operation::LongLimit(1),
        match command {
          0 => Operation::ForgetShort(0),
          1 => Operation::ForgetLong(0),
          2 => Operation::LongFromShort { difference: 0, index: 0 },
          _ => unreachable!(),
        },
      ];
      let next = memory.apply(next.picture()).unwrap();
      assert_eq!(next.references.contains(&old), command == 2);
      assert!(next.references.contains(&next.id));
    }
  }
}

#[test]
fn test_apply_large_gap_retains_only_final_synthetic_references() {
  for maximum in [0, 1, 2, 16] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.modulus = 65536;
    first.maximum = maximum;
    first.idr = true;
    memory.apply(first.picture()).unwrap();
    let mut next = picture(65535);
    next.modulus = 65536;
    next.maximum = maximum;
    let result = memory.apply(next.picture()).unwrap();
    assert_eq!(result.id, 65536);
    assert_eq!(result.references.len(), maximum as usize);
    if maximum != 0 {
      assert_eq!(
        result.references,
        ((65537 - u64::from(maximum))..=65536).collect::<Vec<_>>()
      );
    }
  }
}

#[test]
fn test_apply_gap_cannot_overcommit_a_pool_of_long_term_references() {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 1;
  first.idr = true;
  first.long_idr = true;
  memory.apply(first.picture()).unwrap();
  let mut next = picture(3);
  next.maximum = 1;
  next.reference = false;
  assert!(memory.apply(next.picture()).is_err());
}
#[test]
fn test_sliding_window_retains_only_newest_short_references() {
  let mut memory = Memory::default();
  let a = memory.apply(picture(0).picture()).unwrap();
  let b = memory.apply(picture(1).picture()).unwrap();
  let c = memory.apply(picture(2).picture()).unwrap();
  assert_eq!(a.references, vec![a.id]);
  assert_eq!(b.references, vec![a.id, b.id]);
  assert_eq!(c.references, vec![b.id, c.id]);
}

#[test]
fn test_apply_rejects_adaptive_marking_that_exceeds_reference_capacity()
{
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 1;
  first.idr = true;
  memory.apply(first.picture()).unwrap();
  let mut next = picture(1);
  next.maximum = 1;
  next.adaptive = true;
  assert!(memory.apply(next.picture()).is_err());
}

#[test]
fn test_apply_rejects_sliding_window_without_an_evictable_reference() {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 1;
  first.idr = true;
  first.long_idr = true;
  memory.apply(first.picture()).unwrap();
  let mut next = picture(1);
  next.maximum = 1;
  assert!(memory.apply(next.picture()).is_err());
}
#[test]
fn test_mmco_short_long_and_reset() {
  let mut memory = Memory::default();
  let mut first = picture(0);
  first.maximum = 4;
  let a = memory.apply(first.picture()).unwrap();
  let mut p = picture(1);
  p.maximum = 4;
  p.adaptive = true;
  p.operations = vec![
    Operation::LongLimit(4),
    Operation::LongFromShort { difference: 0, index: 3 },
  ];
  let b = memory.apply(p.picture()).unwrap();
  assert_eq!(b.references, vec![a.id, b.id]);
  let mut p = picture(2);
  p.maximum = 4;
  p.adaptive = true;
  p.operations =
    vec![Operation::ForgetLong(3), Operation::ForgetShort(0)];
  let c = memory.apply(p.picture()).unwrap();
  assert_eq!(c.references, vec![c.id]);
  let mut p = picture(3);
  p.maximum = 4;
  p.adaptive = true;
  p.operations = vec![Operation::Reset];
  let d = memory.apply(p.picture()).unwrap();
  assert_eq!(d.references, vec![d.id]);
}
#[test]
fn test_fields_share_identity_and_disposable_pictures_do_not_become_references()
 {
  let mut memory = Memory::default();
  let mut p = picture(0);
  p.field = Some(false);
  let top = memory.apply(p.picture()).unwrap();
  let mut p = picture(0);
  p.field = Some(true);
  let bottom = memory.apply(p.picture()).unwrap();
  assert_eq!(top.id, bottom.id);
  assert_eq!(bottom.references, vec![top.id]);
  let mut p = picture(1);
  p.reference = false;
  let next = memory.apply(p.picture()).unwrap();
  assert_eq!(next.references, vec![top.id]);
}
#[test]
fn test_frame_wrap_and_long_term_limit() {
  let mut memory = Memory::default();
  let mut first = picture(15);
  first.maximum = 3;
  let a = memory.apply(first.picture()).unwrap();
  let mut p = picture(0);
  p.maximum = 3;
  p.adaptive = true;
  p.operations = vec![
    Operation::LongLimit(3),
    Operation::LongFromShort { difference: 0, index: 2 },
    Operation::CurrentLong(1),
  ];
  let b = memory.apply(p.picture()).unwrap();
  assert_eq!(b.references, vec![a.id, b.id]);
  let mut p = picture(1);
  p.maximum = 3;
  p.adaptive = true;
  p.operations = vec![Operation::LongLimit(2)];
  let c = memory.apply(p.picture()).unwrap();
  assert_eq!(c.references, vec![b.id, c.id]);
}

#[test]
fn test_idr_clears_prior_references_and_zero_capacity_retains_none() {
  let mut memory = Memory::default();
  memory.apply(picture(4).picture()).unwrap();
  let mut idr = picture(0);
  idr.idr = true;
  idr.long_idr = true;
  let state = memory.apply(idr.picture()).unwrap();
  assert_eq!(state.references, vec![state.id]);
  let mut next = picture(1);
  next.adaptive = true;
  next.operations = vec![Operation::ForgetLong(0)];
  let state = memory.apply(next.picture()).unwrap();
  assert_eq!(state.references, vec![state.id]);
  let mut intra = picture(0);
  intra.idr = true;
  intra.maximum = 0;
  assert!(memory.apply(intra.picture()).unwrap().references.is_empty());
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
        let a = memory.apply(first.picture()).unwrap();
        let mut second = picture(0);
        second.field = Some(true);
        second.reference = second_reference;
        second.idr = boundary == 1;
        if boundary == 2 {
          second.adaptive = true;
          second.operations.push(Operation::Reset);
        }
        let b = memory.apply(second.picture()).unwrap();
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
      let old = memory.apply(first.picture()).unwrap().id;
      let mut second = picture(0);
      second.field = Some(!bottom);
      second.adaptive = true;
      second.maximum = 4;
      second.operations.push(Operation::CurrentLong(0));
      assert_eq!(
        memory.apply(second.picture()).unwrap().references,
        vec![old]
      );
      let mut next = picture(1);
      next.maximum = 4;
      next.field = Some(bottom);
      next.adaptive = true;
      if !promote {
        next.operations.push(Operation::CurrentLong(0));
      }
      let mut result = memory.apply(next.picture()).unwrap();
      if promote {
        let mut command = picture(2);
        command.maximum = 4;
        command.field = Some(bottom);
        command.adaptive = true;
        command
          .operations
          .push(Operation::LongFromShort { difference: 1, index: 0 });
        result = memory.apply(command.picture()).unwrap();
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
      let old = memory.apply(picture(start).picture()).unwrap().id;
      let mut next = picture(end);
      next.reference = reference;
      let result = memory.apply(next.picture()).unwrap();
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
  let old = memory.apply(picture(0).picture()).unwrap().id;
  let mut invalid = picture(3);
  invalid.gaps_allowed = false;
  assert!(memory.apply(invalid.picture()).is_err());
  let state = memory.apply(picture(1).picture()).unwrap();
  assert_eq!(state.references, vec![old, state.id]);
}

#[test]
fn test_apply_reset_restarts_gap_tracking_and_field_identity() {
  let mut memory = Memory::default();
  memory.apply(picture(8).picture()).unwrap();
  let mut reset = picture(9);
  reset.adaptive = true;
  reset.operations.push(Operation::Reset);
  let old = memory.apply(reset.picture()).unwrap().id;
  let mut next = picture(1);
  next.gaps_allowed = false;
  let state = memory.apply(next.picture()).unwrap();
  assert_eq!(state.references, vec![old, state.id]);
}

#[test]
fn test_apply_rejects_out_of_domain_marking_before_changing_references()
{
  for operation in [
    Operation::CurrentLong(u32::MAX),
    Operation::LongFromShort { difference: 0, index: 16 },
    Operation::ForgetLong(32),
    Operation::ForgetShort(u32::MAX),
    Operation::LongLimit(17),
  ] {
    let mut memory = Memory::default();
    let old = memory.apply(picture(0).picture()).unwrap().id;
    let mut input = picture(1);
    input.adaptive = true;
    input.operations.push(operation);
    assert!(memory.apply(input.picture()).is_err());
    let next = memory.apply(picture(1).picture()).unwrap();
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
          memory.apply(input.picture()).unwrap();
        }
        let mut first = picture(number);
        first.field = Some(bottom);
        first.maximum = maximum;
        first.gaps_allowed = false;
        first.adaptive = true;
        first.operations.push(Operation::Reset);
        let first = memory.apply(first.picture()).unwrap();
        let mut second = picture(0);
        second.field = Some(!bottom);
        second.maximum = maximum;
        second.gaps_allowed = false;
        let second = memory.apply(second.picture()).unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.continuation);
        assert_eq!(second.references, vec![first.id]);
        let mut next = picture(1);
        next.maximum = maximum;
        next.gaps_allowed = false;
        let next = memory.apply(next.picture()).unwrap();
        assert!(!next.continuation);
        assert_eq!(next.references.contains(&first.id), maximum > 1);
      }
    }
  }
}

#[test]
fn test_apply_rejects_duplicate_and_forbidden_command_combinations() {
  for operations in [
    vec![Operation::LongLimit(1), Operation::LongLimit(1)],
    vec![Operation::Reset, Operation::Reset],
    vec![Operation::CurrentLong(0), Operation::CurrentLong(0)],
    vec![Operation::CurrentLong(0), Operation::Reset],
    vec![Operation::ForgetShort(0), Operation::Reset],
    vec![Operation::Reset, Operation::ForgetLong(0)],
    vec![
      Operation::LongFromShort { difference: 0, index: 0 },
      Operation::Reset,
    ],
  ] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    first.long_idr = true;
    let a = memory.apply(first.picture()).unwrap().id;
    let mut second = picture(1);
    second.maximum = 4;
    let b = memory.apply(second.picture()).unwrap().id;
    let mut invalid = picture(2);
    invalid.maximum = 4;
    invalid.adaptive = true;
    invalid.operations = operations;
    assert!(memory.apply(invalid.picture()).is_err());
    let mut retry = picture(2);
    retry.maximum = 4;
    let next = memory.apply(retry.picture()).unwrap();
    assert_eq!(next.references, [a, b, next.id]);
  }
}

#[test]
fn test_apply_later_commands_cannot_retire_current_long_assignment() {
  for operation in [
    Operation::LongLimit(0),
    Operation::LongFromShort { difference: 0, index: 0 },
  ] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    first.long_idr = true;
    memory.apply(first.picture()).unwrap();
    let mut second = picture(1);
    second.maximum = 4;
    memory.apply(second.picture()).unwrap();
    let mut invalid = picture(2);
    invalid.maximum = 4;
    invalid.adaptive = true;
    invalid.operations = vec![Operation::CurrentLong(0), operation];
    assert!(memory.apply(invalid.picture()).is_err());
  }
}

#[test]
fn test_apply_permitted_command_order_and_current_complement_survive() {
  for operations in [
    vec![Operation::CurrentLong(0), Operation::LongLimit(1)],
    vec![
      Operation::Reset,
      Operation::LongLimit(1),
      Operation::CurrentLong(0),
    ],
  ] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.idr = true;
    first.long_idr = true;
    memory.apply(first.picture()).unwrap();
    let mut next = picture(1);
    next.maximum = 4;
    next.adaptive = true;
    next.operations = operations;
    let next = memory.apply(next.picture()).unwrap();
    assert_eq!(next.references, [next.id]);
  }
  for bottom in [false, true] {
    let mut memory = Memory::default();
    let mut first = picture(0);
    first.maximum = 4;
    first.field = Some(bottom);
    first.idr = true;
    let first = memory.apply(first.picture()).unwrap();
    let mut second = picture(0);
    second.maximum = 4;
    second.field = Some(!bottom);
    second.adaptive = true;
    second.operations = vec![
      Operation::LongLimit(1),
      Operation::CurrentLong(0),
      Operation::LongFromShort { difference: 0, index: 0 },
    ];
    let second = memory.apply(second.picture()).unwrap();
    assert_eq!(second.id, first.id);
    assert!(second.continuation);
    assert_eq!(second.references, [first.id]);
  }
}
