//! module: picture; responsibility: describe coded-picture presentation
//! metadata. operations/entities: Geometry, Descriptor, Boundary,
//! Output, and public semantic plans; tests: backend/layout_tests,
//! frontend_tests and picture/plan_tests. Geometry stays with its
//! submission descriptor; the plan child owns output policy and
//! complementary-field completion semantics.
use crate::timestamp::Source;

mod plan;
#[cfg(test)]
mod plan_tests;
pub use plan::{Boundary, Id, Output, Part, Plan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
  pub width: u32,
  pub height: u32,
  pub coded_width: u32,
  pub coded_height: u32,
  pub left: u32,
  pub top: u32,
}
#[derive(Clone, Debug)]
pub struct Descriptor {
  pub geometry: Option<Geometry>,
  pub plan: Plan,
  pub timestamp: u64,
  pub source: Source,
}
