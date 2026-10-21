//! module: syntax::geometry; responsibility: define decoded picture
//! geometry. operations: Geometry; entities: Geometry; tests:
//! codec-specific geometry tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Geometry {
  pub id: u32,
  pub width: u32,
  pub height: u32,
  pub coded_width: u32,
  pub coded_height: u32,
  pub left: u32,
  pub top: u32,
  pub limits: crate::Limits,
}
