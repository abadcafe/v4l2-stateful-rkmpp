//! module: bitstream::report; responsibility: carry Annex B semantic
//! admission without owning the caller's buffer or timestamp policy.
//! operations/entities: AccessUnit, Report; tests: bitstream_tests.
#[derive(Debug)]
pub struct AccessUnit {
  pub payload: crate::submission::Payload,
  pub geometry: crate::picture::Geometry,
  pub plan: crate::picture::Plan,
}
#[derive(Debug, Default)]
pub struct Report {
  pub units: Vec<AccessUnit>,
  pub end: bool,
}
