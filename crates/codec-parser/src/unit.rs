//! module: codec_parser::unit; responsibility: retain validated input
//! extent and its ordered termination boundary. operations: Admission,
//! Unit, Boundary, annexb, sequence_end, stream_end, payload; entities:
//! Admission, Unit, Boundary; tests: unit_tests and codec admission
//! tests.
use std::io;
/// Admission includes metadata effects even when no coded picture is
/// present.
pub struct Admission {
  pub units: Vec<Unit>,
  /// True establishes that this submission supplies decoder
  /// configuration. False makes no promise about configuration
  /// already present in a task.
  pub configures_decoder: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
  Sequence,
  Stream,
}
pub struct Unit {
  /// The task supplies decoder configuration before its coded picture.
  /// False makes no promise about whether configuration is present.
  pub configures_decoder: bool,
  pub prefix: usize,
  pub header: Option<super::header::Header>,
  pub range: std::ops::Range<usize>,
  pub display: bool,
  pub boundary: Option<Boundary>,
}
impl Unit {
  pub(super) fn annexb(size: usize) -> Self {
    Self {
      configures_decoder: false,
      prefix: size,
      header: None,
      range: 0..size,
      display: true,
      boundary: None,
    }
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
