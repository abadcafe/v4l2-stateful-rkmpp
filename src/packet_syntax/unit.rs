//! module: packet_syntax::unit; responsibility: retain validated input
//! extent and its ordered termination boundary. operations: Unit,
//! Boundary, annexb, sequence_end, stream_end, payload; entities:
//! Unit, Boundary; tests: unit_tests and codec admission tests.
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
  Sequence,
  Stream,
}
pub(crate) struct Unit {
  pub range: std::ops::Range<usize>,
  pub display: bool,
  pub boundary: Option<Boundary>,
}
impl Unit {
  pub(super) fn annexb(size: usize) -> Self {
    Self { range: 0..size, display: true, boundary: None }
  }

  pub(super) fn sequence_end(
    &mut self,
    offset: usize,
  ) -> io::Result<()> {
    self.payload()?;
    self.range.end = offset;
    self.boundary = Some(Boundary::Sequence);
    Ok(())
  }

  pub(super) fn stream_end(&mut self, offset: usize) -> io::Result<()> {
    if self.boundary == Some(Boundary::Stream) {
      return Err(invalid());
    }
    if self.boundary.is_none() {
      self.range.end = offset;
    }
    self.boundary = Some(Boundary::Stream);
    Ok(())
  }

  pub(super) fn payload(&self) -> io::Result<()> {
    if self.boundary.is_some() { Err(invalid()) } else { Ok(()) }
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid access-unit termination order",
  )
}
