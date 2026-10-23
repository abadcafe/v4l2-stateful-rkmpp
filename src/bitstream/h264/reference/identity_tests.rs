//! module: reference::identity_tests; responsibility: define coded
//! picture identity independently of DPB storage. operations: test
//! cases; entities: none; tests: IDR origin, consecutive complementary
//! fields, serial lifetime.
use super::super::number::Number;
use super::identity::{Identity, Picture};

fn picture(
  frame: u32,
  field: Option<bool>,
  reference: bool,
  idr: bool,
) -> Picture {
  Picture {
    number: Number::new(frame, 4).unwrap(),
    field,
    reference,
    idr,
    reset: false,
  }
}

#[test]
fn test_check_idr_requires_zero_without_changing_live_identity() {
  for field in [None, Some(false), Some(true)] {
    let mut identity = Identity::default();
    let first = picture(0, field, true, true);
    let assigned = identity.begin(first).unwrap();
    identity.finish(first, &assigned, None);
    for frame in [1, 15] {
      assert!(
        identity.check(picture(frame, field, true, true)).is_err()
      );
    }
    assert_eq!(identity.previous(), Some(0));
    assert_eq!(
      identity.begin(picture(1, None, true, false)).unwrap().id,
      assigned.id + 1
    );
  }
}

#[test]
fn test_check_repeated_number_requires_opposite_reference_fields() {
  for first in [None, Some(false), Some(true)] {
    for second in [None, Some(false), Some(true)] {
      for reference in [false, true] {
        let mut identity = Identity::default();
        let p = picture(0, first, true, true);
        let assigned = identity.begin(p).unwrap();
        identity.finish(p, &assigned, None);
        let next = picture(0, second, reference, false);
        let paired = reference
          && first.is_some()
          && second.is_some()
          && first != second;
        assert_eq!(identity.check(next).is_ok(), paired);
        if paired {
          let next = identity.begin(next).unwrap();
          assert!(next.continuation);
          assert_eq!(next.id, assigned.id);
        }
      }
    }
  }
}

#[test]
fn test_begin_pair_cannot_admit_a_third_field_or_nonconsecutive_complement()
 {
  for bottom in [false, true] {
    for interrupted in [false, true] {
      let mut identity = Identity::default();
      let first = picture(0, Some(bottom), true, true);
      let assigned = identity.begin(first).unwrap();
      identity.finish(first, &assigned, Some(0));
      if interrupted {
        let middle = picture(1, None, false, false);
        let assigned = identity.begin(middle).unwrap();
        identity.finish(middle, &assigned, None);
        assert!(
          identity
            .check(picture(0, Some(!bottom), true, false))
            .is_err()
        );
      } else {
        let second = picture(0, Some(!bottom), true, false);
        let other = identity.begin(second).unwrap();
        assert_eq!(other.id, assigned.id);
        assert_eq!(other.required_long, Some(0));
        identity.finish(second, &other, Some(0));
        assert!(
          identity
            .check(picture(0, Some(bottom), true, false))
            .is_err()
        );
      }
    }
  }
}

#[test]
fn test_reset_and_missing_frames_preserve_serial_and_normalize_mmco5() {
  let mut identity = Identity::default();
  identity.skip(500).unwrap();
  let generated =
    identity.missing(Number::new(14, 4).unwrap()).unwrap();
  assert_eq!(generated, 501);
  assert_eq!(identity.previous(), Some(14));
  identity.reset();
  assert_eq!(identity.previous(), None);
  let mut first = picture(1, Some(false), true, false);
  first.reset = true;
  let assigned = identity.begin(first).unwrap();
  identity.finish(first, &assigned, None);
  assert_eq!(assigned.id, 502);
  assert_eq!(identity.previous(), Some(0));
  let other =
    identity.begin(picture(0, Some(true), true, false)).unwrap();
  assert!(other.continuation);
  assert_eq!(other.id, assigned.id);
}
