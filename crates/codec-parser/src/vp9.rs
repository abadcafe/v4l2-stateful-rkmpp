//! module: codec_parser::vp9; responsibility: validate VP9 superframes
//! and frame-header bounds. operations: new, validate;
//! entities: Validator; tests: vp9_tests.
use std::io;

mod native;
#[cfg(test)]
mod native_tests;

pub(crate) struct Validator {
  parser: native::Parser,
  needs_keyframe: bool,
}
impl Validator {
  pub(crate) fn new() -> io::Result<Self> {
    Ok(Self { parser: native::Parser::new()?, needs_keyframe: true })
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<Vec<super::unit::Unit>> {
    let result = self.packet(bytes, budget);
    if result.is_err() {
      self.needs_keyframe = true;
    }
    result
  }

  fn packet(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<Vec<super::unit::Unit>> {
    if bytes.is_empty() {
      return Err(invalid());
    }
    let index = self.parser.index(bytes)?;
    if index.count as usize > budget {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "coded frame budget exhausted",
      ));
    }
    let mut offset = 0usize;
    let mut ranges = Vec::new();
    for &size in
      index.sizes.get(..index.count as usize).ok_or_else(invalid)?
    {
      let end =
        offset.checked_add(size as usize).ok_or_else(invalid)?;
      let frame = bytes
        .get(offset..end)
        .filter(|b| !b.is_empty())
        .ok_or_else(invalid)?;
      let header = self.parser.frame(frame)?;
      if self.needs_keyframe && (header.existing || !header.key_frame) {
        return Err(invalid());
      }
      let headers = u64::from(header.header_bytes)
        + u64::from(header.partition_bytes);
      if headers > frame.len() as u64
        || (!header.existing && headers == frame.len() as u64)
      {
        return Err(invalid());
      }
      self.needs_keyframe = false;
      ranges.push(super::unit::Unit {
        prefix: 0,
        header: None,
        boundary: None,
        range: offset..end,
        display: header.display,
      });
      offset = end;
    }
    if offset + index.index_bytes as usize != bytes.len() {
      return Err(invalid());
    }
    Ok(ranges)
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid VP9 frame header or superframe",
  )
}
