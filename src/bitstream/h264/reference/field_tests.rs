//! module: reference::field_tests; responsibility: define
//! complementary-field marking constraints through field views,
//! independently of DPB storage. operations: test cases; entities:
//! none; tests: assignment agreement, required promotion and
//! replacement of a short complement.
use codec_parser::header::Operation;

use super::super::number::Number;
use super::field::{Current, Promotion, Reference, complete, promote};

#[test]
fn test_complete_requires_first_field_long_assignment_even_after_removal()
 {
  for assigned in [None, Some(0), Some(1)] {
    assert_eq!(
      complete(
        Current {
          id: 3,
          field: true,
          required_long: Some(0),
          assigned_long: assigned
        },
        []
      )
      .is_ok(),
      assigned == Some(0)
    );
  }
}

#[test]
fn test_complete_pair_assignments_agree_with_the_retained_complement() {
  for index in [0, 1] {
    let reference = Reference { id: 3, long: Some(0) };
    assert_eq!(
      complete(
        Current {
          id: 3,
          field: true,
          required_long: None,
          assigned_long: Some(index)
        },
        [reference]
      )
      .is_ok(),
      index == 0
    );
  }
}

#[test]
fn test_complete_current_long_requires_short_complement_to_be_promoted_or_removed()
 {
  let current = Current {
    id: 3,
    field: true,
    required_long: None,
    assigned_long: Some(0),
  };
  assert!(
    complete(current, [Reference { id: 3, long: None }]).is_err()
  );
  assert!(complete(current, []).is_ok());
  assert!(
    complete(current, [Reference { id: 3, long: Some(0) }]).is_ok()
  );
  assert!(complete(current, [Reference { id: 2, long: None }]).is_ok());
}

#[test]
fn test_promote_requires_matching_complement_assignment_even_after_removal()
 {
  for remove in [false, true] {
    for other in [false, true] {
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
      assert_eq!(
        promote(
          Promotion {
            number: Number::new(1, 4).unwrap(),
            target: 1,
            index: 0,
            short_complement: true,
            current_pair: false
          },
          &operations
        )
        .is_ok(),
        other
      );
    }
  }
  let p = Promotion {
    number: Number::new(0, 4).unwrap(),
    target: 0,
    index: 0,
    short_complement: false,
    current_pair: true,
  };
  assert!(promote(p, &[]).is_err());
  assert!(promote(p, &[Operation::CurrentLong(0)]).is_ok());
  assert!(promote(p, &[Operation::CurrentLong(1)]).is_err());
  assert!(promote(Promotion { current_pair: false, ..p }, &[]).is_ok());
}

#[test]
fn test_promote_extreme_difference_and_target_do_not_overflow() {
  for frame in [0, 65535] {
    for target in [i64::MIN, -1, 0, i64::MAX] {
      assert!(
        promote(
          Promotion {
            number: Number::new(frame, 16).unwrap(),
            target,
            index: 0,
            short_complement: true,
            current_pair: false,
          },
          &[Operation::LongFromShort {
            difference: u32::MAX,
            index: 0
          }]
        )
        .is_err()
      );
    }
  }
}
