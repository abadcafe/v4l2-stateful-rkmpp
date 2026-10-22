//! module: vp8::native; responsibility: own SDK parsing behind a
//! bounded Rust interface. operations: new, frame, reset;
//! entities: Parser, Frame; tests: native_tests. Input borrows end at
//! the synchronous call; returned facts contain no SDK pointers.
use std::io;

use crate::ffi;

pub(super) struct Frame {
  pub header_bits: u32,
  pub partition_bytes: u32,
  pub width: u32,
  pub height: u32,
  pub key_frame: bool,
  pub display: bool,
}

pub(super) struct Parser(Box<ffi::GstVp8Parser>);
impl Parser {
  pub(super) fn new() -> Self {
    let mut parser = Self(Box::default());
    parser.reset();
    parser
  }

  pub(super) fn frame(&mut self, bytes: &[u8]) -> io::Result<Frame> {
    let mut frame = ffi::GstVp8FrameHdr::default();
    let size = bytes.len().try_into().map_err(|error| {
      io::Error::new(io::ErrorKind::InvalidInput, error)
    })?;
    // SAFETY: this exclusive owner and initialized output remain live
    // through the call; bytes is borrowed only during that call.
    let status = unsafe {
      ffi::gst_vp8_parser_parse_frame_header(
        &mut *self.0,
        &mut frame,
        bytes.as_ptr(),
        size,
      )
    };
    if status != 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("native VP8 frame header: {status}"),
      ));
    }
    Ok(Frame {
      header_bits: frame.header_size,
      partition_bytes: frame.first_part_size,
      width: u32::from(frame.width),
      height: u32::from(frame.height),
      key_frame: frame.key_frame != 0,
      display: frame.show_frame != 0,
    })
  }

  pub(super) fn reset(&mut self) {
    // SAFETY: the owner is live and exclusively borrowed.
    unsafe { ffi::gst_vp8_parser_init(&mut *self.0) };
  }
}
