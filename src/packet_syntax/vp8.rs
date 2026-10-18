//! module: packet_syntax::vp8; responsibility: validate VP8 frame
//! headers and partition bounds across a sequence. operations: new,
//! validate; entities: Validator; tests: vp8_tests.
use std::io;

use super::ffi;
pub(crate) struct Validator {
  parser: ffi::GstVp8Parser,
  needs_keyframe: bool,
}
impl Validator {
  pub(crate) fn new() -> io::Result<Self> {
    // SAFETY: this public C value is initialized by the parser API.
    let mut parser = unsafe { std::mem::zeroed() };
    unsafe { ffi::gst_vp8_parser_init(&mut parser) };
    Ok(Self { parser, needs_keyframe: true })
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
  ) -> io::Result<super::unit::Unit> {
    let valid_tag = bytes.len() >= 3 && (bytes[0] >> 1) & 7 <= 3;
    let key_sync = bytes.first().is_some_and(|tag| tag & 1 != 0)
      || (bytes.len() >= 10 && bytes[3..6] == [0x9d, 1, 0x2a]);
    if !valid_tag
      || !key_sync
      || (self.needs_keyframe && bytes[0] & 1 != 0)
    {
      self.needs_keyframe = true;
      return Err(invalid());
    }
    // SAFETY: parser and output are owned C values; the slice is live
    // throughout the synchronous call and is never retained.
    let mut header: ffi::GstVp8FrameHdr = unsafe { std::mem::zeroed() };
    let status = unsafe {
      ffi::gst_vp8_parser_parse_frame_header(
        &mut self.parser,
        &mut header,
        bytes.as_ptr(),
        bytes.len() as _,
      )
    };
    if status != 0
      || u64::from(header.header_size)
        > u64::from(header.first_part_size) * 8
      || (header.key_frame != 0
        && (header.width == 0 || header.height == 0))
    {
      unsafe { ffi::gst_vp8_parser_init(&mut self.parser) };
      self.needs_keyframe = true;
      return Err(invalid());
    }
    self.needs_keyframe = false;
    Ok(super::unit::Unit {
      prefix: 0,
      header: None,
      boundary: None,
      range: 0..bytes.len(),
      display: header.show_frame != 0,
    })
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid VP8 frame header or partition",
  )
}
