//! module: bitstream::h264::reference_field_tests; responsibility:
//! define complementary-field marking admission. operations: public
//! Memory apply; entities: none; tests: first-field assignment,
//! identical pair indices, short complement and promotion obligation.
use codec_parser::header::Operation;

use super::reference::{Marking, Memory, Picture};
fn field(
  frame: u32,
  bottom: bool,
  marking: Marking<'_>,
) -> Picture<'_> {
  Picture {
    frame,
    modulus: 16,
    field: Some(bottom),
    maximum: 4,
    gaps_allowed: true,
    marking,
  }
}

#[test]
fn test_apply_second_field_must_repeat_first_long_assignment_even_after_removal()
 {
  for bottom in [false, true] {
    for remove in [false, true] {
      for assign in [false, true] {
        let mut memory = Memory::default();
        let first = memory
          .apply(field(0, bottom, Marking::Idr { long: true }))
          .unwrap();
        let mut operations = Vec::new();
        if remove {
          operations.push(Operation::ForgetLong(0));
        }
        if assign {
          operations.push(Operation::CurrentLong(0));
        }
        let result = memory.apply(field(
          0,
          !bottom,
          Marking::Adaptive(&operations),
        ));
        assert_eq!(result.is_ok(), assign);
        if let Ok(result) = result {
          assert_eq!(result.id, first.id);
          assert!(result.continuation);
          assert_eq!(result.references, [first.id]);
        }
      }
    }
  }
}

#[test]
fn test_apply_pair_long_assignments_must_use_the_same_index() {
  for bottom in [false, true] {
    for index in [0, 1] {
      let mut memory = Memory::default();
      memory
        .apply(field(0, bottom, Marking::Idr { long: true }))
        .unwrap();
      let result = memory.apply(field(
        0,
        !bottom,
        Marking::Adaptive(&[
          Operation::LongLimit(2),
          Operation::CurrentLong(index),
        ]),
      ));
      assert_eq!(result.is_ok(), index == 0);
    }
  }
}

#[test]
fn test_apply_current_long_requires_short_complement_to_be_promoted_or_removed()
 {
  for bottom in [false, true] {
    for action in 0..3 {
      let mut memory = Memory::default();
      memory
        .apply(field(0, bottom, Marking::Idr { long: false }))
        .unwrap();
      let mut operations = vec![Operation::LongLimit(1)];
      if action == 1 {
        operations.push(Operation::ForgetShort(0));
      } else if action == 2 {
        operations
          .push(Operation::LongFromShort { difference: 0, index: 0 });
      }
      operations.push(Operation::CurrentLong(0));
      let result =
        memory.apply(field(0, !bottom, Marking::Adaptive(&operations)));
      assert_eq!(result.is_ok(), action != 0);
    }
  }
}

#[test]
fn test_apply_promotion_requires_a_command_for_the_short_complement() {
  for bottom in [false, true] {
    for remove in [false, true] {
      for other in [false, true] {
        let mut memory = Memory::default();
        let mut first = field(0, bottom, Marking::Idr { long: false });
        first.field = None;
        memory.apply(first).unwrap();
        let mut operations = vec![
          Operation::LongLimit(1),
          Operation::LongFromShort { difference: 1, index: 0 },
        ];
        if remove {
          operations.push(Operation::ForgetLong(1));
        }
        if other {
          operations
            .push(Operation::LongFromShort { difference: 2, index: 0 });
        }
        let result = memory.apply(field(
          1,
          bottom,
          Marking::Adaptive(&operations),
        ));
        assert_eq!(result.is_ok(), other);
      }
    }
  }
}
