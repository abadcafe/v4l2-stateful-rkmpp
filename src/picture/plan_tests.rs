//! module: picture::plan_tests; responsibility: immutable semantic
//! plans.
use codec_parser::Limits;

use super::{Boundary, Id, Output, Part, Plan};
#[test]
fn test_complete_pair_combines_picture_order_and_final_reference_state()
{
  let mut first = Plan::new(
    Id::new(1),
    Part::FirstField,
    vec![Id::new(1)],
    Limits::default(),
    true,
    Output {
      order: 9,
      before: Boundary::Discard,
      ..Default::default()
    },
  );
  let second = Plan::new(
    Id::new(1),
    Part::Continuation,
    vec![Id::new(2)],
    Limits::default(),
    true,
    Output { order: 8, ..Default::default() },
  );
  first.complete_pair(&second).unwrap();
  assert_eq!(first.references(), &[Id::new(2)]);
  assert_eq!(first.output().order, 8);
  assert_eq!(first.output().before, Boundary::Discard);
  let unrelated = Plan::new(
    Id::new(3),
    Part::Continuation,
    vec![],
    Limits::default(),
    true,
    Output::default(),
  );
  assert!(first.complete_pair(&unrelated).is_err());
}

#[test]
fn test_complete_pair_rejects_invalid_field_roles_without_mutation() {
  for (first_part, second_part) in [
    (Part::Frame, Part::Continuation),
    (Part::Continuation, Part::Continuation),
    (Part::FirstField, Part::Frame),
    (Part::FirstField, Part::FirstField),
  ] {
    let mut first = Plan::new(
      Id::new(1),
      first_part,
      vec![Id::new(2)],
      Limits::default(),
      true,
      Output {
        order: 9,
        before: Boundary::Discard,
        ..Output::default()
      },
    );
    let second = Plan::new(
      Id::new(1),
      second_part,
      vec![Id::new(3)],
      Limits::default(),
      false,
      Output { order: 3, ..Output::default() },
    );
    assert!(first.complete_pair(&second).is_err());
    assert_eq!(first.id(), Id::new(1));
    assert_eq!(first.part(), first_part);
    assert_eq!(first.references(), &[Id::new(2)]);
    assert_eq!(first.output().order, 9);
    assert_eq!(first.output().before, Boundary::Discard);
    assert!(first.reference());
  }
}
