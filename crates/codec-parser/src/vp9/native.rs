//! module: vp9::native; responsibility: own SDK parsing behind a
//! bounded Rust interface. operations: new, index, frame, drop;
//! entities: Parser, Index, Frame; tests: native_tests. No returned
//! facts borrow SDK storage or compressed input.
use std::io;
use std::ptr::NonNull;

use crate::ffi;

pub(super) struct Frame {
  pub header_bytes: u32,
  pub partition_bytes: u32,
  pub key_frame: bool,
  pub existing: bool,
  pub display: bool,
}
pub(super) struct Index {
  pub count: u32,
  pub index_bytes: u32,
  pub sizes: [u32; 8],
}

pub(super) struct Parser(NonNull<ffi::GstVp9Parser>);
impl Parser {
  pub(super) fn new() -> io::Result<Self> {
    // SAFETY: success transfers one initialized native owner.
    NonNull::new(unsafe { ffi::gst_vp9_parser_new() })
      .map(Self)
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::OutOfMemory,
          "allocate native VP9 parser",
        )
      })
  }

  pub(super) fn index(&mut self, bytes: &[u8]) -> io::Result<Index> {
    let mut index = ffi::GstVp9SuperframeInfo::default();
    let size = bytes.len().try_into().map_err(|error| {
      io::Error::new(io::ErrorKind::InvalidInput, error)
    })?;
    // SAFETY: this exclusive owner and initialized output remain live
    // through the call; bytes is borrowed only during that call.
    let status = unsafe {
      ffi::gst_vp9_parser_parse_superframe_info(
        self.0.as_ptr(),
        &mut index,
        bytes.as_ptr(),
        size,
      )
    };
    if status != 0 {
      return Err(invalid("superframe", status));
    }
    Ok(Index {
      count: index.frames_in_superframe,
      index_bytes: index.superframe_index_size,
      sizes: index.frame_sizes,
    })
  }

  pub(super) fn frame(&mut self, bytes: &[u8]) -> io::Result<Frame> {
    let mut frame = ffi::GstVp9FrameHdr::default();
    let size = bytes.len().try_into().map_err(|error| {
      io::Error::new(io::ErrorKind::InvalidInput, error)
    })?;
    // SAFETY: the unique owner, output and input borrow remain live
    // until the synchronous native parse finishes.
    let status = unsafe {
      ffi::gst_vp9_parser_parse_frame_header(
        self.0.as_ptr(),
        &mut frame,
        bytes.as_ptr(),
        size,
      )
    };
    if status != 0 {
      return Err(invalid("frame header", status));
    }
    Ok(Frame {
      header_bytes: frame.frame_header_length_in_bytes,
      partition_bytes: frame.first_partition_size,
      key_frame: frame.show_existing_frame == 0
        && frame.frame_type == 0,
      existing: frame.show_existing_frame != 0,
      display: frame.show_frame != 0 || frame.show_existing_frame != 0,
    })
  }
}
impl Drop for Parser {
  fn drop(&mut self) {
    // SAFETY: this is the unique native owner; no input is retained.
    unsafe { ffi::gst_vp9_parser_free(self.0.as_ptr()) };
  }
}
fn invalid(operation: &str, status: u32) -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    format!("native VP9 {operation}: {status}"),
  )
}
