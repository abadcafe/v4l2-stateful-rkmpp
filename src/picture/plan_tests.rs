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
