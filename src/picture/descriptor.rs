//! module: picture::descriptor; responsibility: bind a coded submission
//! to its presentation metadata. operations/entities: Descriptor,
//! Geometry; tests: backend/layout_tests and frontend_tests.
use super::plan::Plan;
use crate::timestamp::Source;
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
