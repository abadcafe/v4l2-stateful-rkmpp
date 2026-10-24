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
  pub(super) fn annexb(
    bytes: &[u8],
    header_bytes: usize,
  ) -> io::Result<Self> {
    // A byte-stream identifier can report NO_NAL_END when the final
    // start code has no header yet. Our input is already client-framed:
    // that prefix cannot become part of the preceding parameter set.
    // Check only the suffix; native identification scans the interior.
    let tail = &bytes[bytes.len().saturating_sub(2 + header_bytes)..];
    if tail.windows(3).any(|window| window == [0, 0, 1]) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "incomplete Annex B NAL header",
      ));
    }
    let size = bytes.len();
    Ok(Self {
      configures_decoder: false,
      prefix: size,
      header: None,
      range: 0..size,
      display: true,
      boundary: None,
    })
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
