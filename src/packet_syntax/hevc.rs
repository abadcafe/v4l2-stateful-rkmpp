//! module: packet_syntax::hevc; responsibility: validate one
//! client-framed HEVC AU or metadata packet. operations: new, validate;
//! entities: Validator; tests: hevc_tests.
use std::io;
use std::ptr::NonNull;

use super::ffi;
mod picture;
#[cfg(test)]
mod picture_tests;
use picture::Picture;
pub(super) struct Validator(NonNull<ffi::GstH265Parser>);
impl Validator {
  pub(super) fn new() -> io::Result<Self> {
    NonNull::new(unsafe { ffi::gst_h265_parser_new() })
      .map(Self)
      .ok_or_else(invalid)
  }

  pub(super) fn validate(
    &mut self,
    bytes: &[u8],
  ) -> io::Result<super::unit::Unit> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
      return Err(invalid());
    }
    let mut offset = 0;
    let mut picture = None;
    let mut unit = super::unit::Unit::annexb(bytes.len());
    while offset < bytes.len() {
      if bytes[offset..].iter().all(|&b| b == 0) {
        break;
      }
      let mut nal: ffi::GstH265NalUnit = unsafe { std::mem::zeroed() };
      let status = unsafe {
        ffi::gst_h265_parser_identify_nalu(
          self.0.as_ptr(),
          bytes.as_ptr(),
          offset as _,
          bytes.len() as _,
          &mut nal,
        )
      };
      if !matches!(status, 0 | 5)
        || nal.size < 2
        || bytes[offset..nal.sc_offset as usize].iter().any(|&b| b != 0)
      {
        return Err(invalid());
      }
      offset = nal.offset as usize + nal.size as usize;
      if offset > bytes.len()
        || bytes[nal.offset as usize] & 0x80 != 0
        || nal.layer_id != 0
        || nal.temporal_id_plus1 == 0
      {
        return Err(invalid());
      }
      let kind = nal.type_;
      match kind {
        36 => unit.sequence_end(nal.sc_offset as usize)?,
        37 => unit.stream_end(nal.sc_offset as usize)?,
        _ => unit.payload()?,
      }
      match kind {
        0..=31 => {
          let mut slice: ffi::GstH265SliceHdr =
            unsafe { std::mem::zeroed() };
          let status = unsafe {
            ffi::gst_h265_parser_parse_slice_hdr(
              self.0.as_ptr(),
              &mut nal,
              &mut slice,
            )
          };
          let current = Picture::new(&nal, &slice);
          let valid = status == 0
            && !slice.pps.is_null()
            && (slice.header_size as usize) < nal.size as usize * 8
            && match &picture {
              None => {
                slice.first_slice_segment_in_pic_flag != 0
                  && slice.dependent_slice_segment_flag == 0
              }
              Some(previous) => {
                slice.first_slice_segment_in_pic_flag == 0
                  && current
                    .as_ref()
                    .is_ok_and(|current| current.agrees(previous))
              }
            };
          unsafe { ffi::gst_h265_slice_hdr_free(&mut slice) };
          if !valid {
            return Err(invalid());
          }
          if picture.is_none() {
            picture = Some(current?);
          }
        }
        32..=34
          if picture.is_some()
            || unsafe {
              ffi::gst_h265_parser_parse_nal(self.0.as_ptr(), &mut nal)
            } != 0 =>
        {
          return Err(invalid());
        }
        35 | 39 if picture.is_some() => return Err(invalid()),
        _ => {}
      }
    }
    if offset == 0 { Err(invalid()) } else { Ok(unit) }
  }
}
impl Drop for Validator {
  fn drop(&mut self) {
    unsafe { ffi::gst_h265_parser_free(self.0.as_ptr()) };
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid framed HEVC access unit",
  )
}
