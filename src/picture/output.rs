//! module: picture::output; responsibility: describe observable output
//! boundaries. operations: Boundary, Output; entities: those value
//! contracts; tests: plan_tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Boundary {
  #[default]
  Continue,
  Drain,
  Discard,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
  pub order: i32,
  pub display: bool,
  pub before: Boundary,
  pub drain_after: bool,
}
