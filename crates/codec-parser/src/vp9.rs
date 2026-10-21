//! module: codec_parser::vp9; responsibility: validate VP9 superframes
//! and frame-header bounds. operations: new, validate;
//! entities: Validator; tests: vp9_tests.
use std::io;
use std::ptr::NonNull;

use super::ffi;
pub(crate) struct Validator {
  parser: NonNull<ffi::GstVp9Parser>,
  needs_keyframe: bool,
}
impl Validator {
  pub(crate) fn new() -> io::Result<Self> {
    let parser = NonNull::new(unsafe { ffi::gst_vp9_parser_new() })
      .ok_or_else(|| io::Error::other("allocate VP9 parser"))?;
    Ok(Self { parser, needs_keyframe: true })
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
    // SAFETY: all buffers are owned and live through synchronous parser
    // calls.
    let mut index: ffi::GstVp9SuperframeInfo = Default::default();
    if unsafe {
      ffi::gst_vp9_parser_parse_superframe_info(
        self.parser.as_ptr(),
        &mut index,
        bytes.as_ptr(),
        bytes.len() as _,
      )
    } != 0
    {
      return Err(invalid());
    }
    if index.frames_in_superframe as usize > budget {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "coded frame budget exhausted",
      ));
    }
    let mut offset = 0usize;
    let mut ranges = Vec::new();
    for &size in index
      .frame_sizes
      .get(..index.frames_in_superframe as usize)
      .ok_or_else(invalid)?
    {
      let end =
        offset.checked_add(size as usize).ok_or_else(invalid)?;
      let frame = bytes
        .get(offset..end)
        .filter(|b| !b.is_empty())
        .ok_or_else(invalid)?;
      let mut header: ffi::GstVp9FrameHdr = Default::default();
      if unsafe {
        ffi::gst_vp9_parser_parse_frame_header(
          self.parser.as_ptr(),
          &mut header,
          frame.as_ptr(),
          frame.len() as _,
        )
      } != 0
      {
        return Err(invalid());
      }
      if self.needs_keyframe
        && (header.show_existing_frame != 0 || header.frame_type != 0)
      {
        return Err(invalid());
      }
      let headers = u64::from(header.frame_header_length_in_bytes)
        + u64::from(header.first_partition_size);
      if headers > frame.len() as u64
        || (header.show_existing_frame == 0
          && headers == frame.len() as u64)
      {
        return Err(invalid());
      }
      self.needs_keyframe = false;
      ranges.push(super::unit::Unit {
        prefix: 0,
        header: None,
        boundary: None,
        range: offset..end,
        display: header.show_frame != 0
          || header.show_existing_frame != 0,
      });
      offset = end;
    }
    if offset + index.superframe_index_size as usize != bytes.len() {
      return Err(invalid());
    }
    Ok(ranges)
  }
}
impl Drop for Validator {
  fn drop(&mut self) {
    unsafe { ffi::gst_vp9_parser_free(self.parser.as_ptr()) };
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid VP9 frame header or superframe",
  )
}
