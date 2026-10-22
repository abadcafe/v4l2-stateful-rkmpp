//! module: codec_parser::vp8; responsibility: validate VP8 frame
//! headers and partition bounds across a sequence. operations: new,
//! validate; entities: Validator; tests: vp8_tests.
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
    Ok(Self { parser: native::Parser::new(), needs_keyframe: true })
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<super::unit::Unit> {
    let valid_tag = bytes.len() >= 3 && (bytes[0] >> 1) & 7 <= 3;
    let key_sync = bytes.first().is_some_and(|tag| tag & 1 != 0)
      || (bytes.len() >= 10 && bytes[3..6] == [0x9d, 1, 0x2a]);
    if budget == 0
      || !valid_tag
      || !key_sync
      || (self.needs_keyframe && bytes[0] & 1 != 0)
    {
      self.needs_keyframe = true;
      return Err(invalid());
    }
    let header = match self.parser.frame(bytes) {
      Ok(header)
        if u64::from(header.header_bits)
          <= u64::from(header.partition_bytes) * 8
          && (!header.key_frame
            || header.width != 0 && header.height != 0) =>
      {
        header
      }
      _ => {
        self.parser.reset();
        self.needs_keyframe = true;
        return Err(invalid());
      }
    };
    self.needs_keyframe = false;
    Ok(super::unit::Unit {
      configures_decoder: false,
      prefix: 0,
      header: None,
      boundary: None,
      range: 0..bytes.len(),
      display: header.display,
    })
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid VP8 frame header or partition",
  )
}
