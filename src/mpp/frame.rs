//! module: mpp::frame; responsibility: own decoded frame leases and
//! borrow their images. operations: from_raw, info, plane, pool_index;
//! entities: Frame, FrameInfo; tests: frame_tests, tests/service.
use std::ffi::{c_char, c_void};
use std::io;
use std::os::fd::BorrowedFd;
use std::ptr::NonNull;

use crate::image::Plane;
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_frame_deinit(frame: *mut *mut c_void) -> i32;
  fn mpp_frame_get_width(frame: *mut c_void) -> u32;
  fn mpp_frame_get_height(frame: *mut c_void) -> u32;
  fn mpp_frame_get_hor_stride(frame: *mut c_void) -> u32;
  fn mpp_frame_get_ver_stride(frame: *mut c_void) -> u32;
  fn mpp_frame_get_buf_size(frame: *mut c_void) -> usize;
  fn mpp_frame_get_mode(frame: *mut c_void) -> u32;
  fn mpp_frame_get_dts(frame: *mut c_void) -> i64;
  fn mpp_frame_get_pts(frame: *mut c_void) -> i64;
  fn mpp_frame_get_errinfo(frame: *mut c_void) -> u32;
  fn mpp_frame_get_discard(frame: *mut c_void) -> u32;
  fn mpp_frame_get_eos(frame: *mut c_void) -> u32;
  fn mpp_frame_get_info_change(frame: *mut c_void) -> u32;
  fn mpp_frame_get_fmt(frame: *mut c_void) -> u32;
  fn mpp_frame_get_buffer(frame: *mut c_void) -> *mut c_void;
  fn mpp_frame_get_color_primaries(frame: *mut c_void) -> u32;
  fn mpp_frame_get_colorspace(frame: *mut c_void) -> u32;
  fn mpp_frame_get_color_trc(frame: *mut c_void) -> u32;
  fn mpp_frame_get_color_range(frame: *mut c_void) -> u32;
  fn mpp_buffer_get_fd_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_get_index_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
}
#[derive(Debug)]
pub struct Frame {
  handle: NonNull<c_void>,
  info: FrameInfo,
}
// SAFETY: Frame transfers exclusive ownership; MPP frame/buffer
// refcounts support cross-thread release. No reference to the frame is
// shared between the session and conversion worker.
unsafe impl Send for Frame {}
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
  pub width: u32,
  pub height: u32,
  pub stride: u32,
  pub rows: u32,
  pub size: usize,
  pub format: u32,
  pub timestamp: u64,
  pub timestamp_source: u32,
  pub field: u32,
  pub eos: bool,
  pub info_change: bool,
  pub failed: bool,
  pub primaries: u32,
  pub matrix: u32,
  pub transfer: u32,
  pub range: u32,
}
impl Frame {
  pub(super) unsafe fn from_raw(handle: *mut c_void) -> Option<Self> {
    NonNull::new(handle).map(|handle| Self {
      info: Self::describe(handle.as_ptr()),
      handle,
    })
  }

  pub fn info(&self) -> FrameInfo {
    self.info
  }

  fn describe(frame: *mut c_void) -> FrameInfo {
    // SAFETY: this owner holds the frame for every getter, with no
    // simultaneous mutation.
    unsafe {
      FrameInfo {
        width: mpp_frame_get_width(frame),
        height: mpp_frame_get_height(frame),
        stride: mpp_frame_get_hor_stride(frame),
        rows: mpp_frame_get_ver_stride(frame),
        size: mpp_frame_get_buf_size(frame),
        format: mpp_frame_get_fmt(frame) & !0x0c000000,
        timestamp: mpp_frame_get_pts(frame) as u64,
        timestamp_source: mpp_frame_get_dts(frame) as u32,
        field: match mpp_frame_get_mode(frame) & 15 {
          1 => 3,
          2 => 4,
          7 => 1,
          11 => 2,
          3 | 15 => 5,
          _ => 0,
        },
        eos: mpp_frame_get_eos(frame) != 0,
        info_change: mpp_frame_get_info_change(frame) != 0,
        failed: mpp_frame_get_errinfo(frame) != 0
          || mpp_frame_get_discard(frame) != 0,
        primaries: mpp_frame_get_color_primaries(frame),
        matrix: mpp_frame_get_colorspace(frame),
        transfer: mpp_frame_get_color_trc(frame),
        range: mpp_frame_get_color_range(frame),
      }
    }
  }

  pub fn plane(&self) -> io::Result<Option<Plane<'_>>> {
    let buffer = unsafe { mpp_frame_get_buffer(self.handle.as_ptr()) };
    if buffer.is_null() {
      return Ok(None);
    }
    let fd = unsafe {
      mpp_buffer_get_fd_with_caller(
        buffer,
        c"v4l2-stateful-rkmpp".as_ptr(),
      )
    };
    if fd < 0 {
      return Err(io::Error::other("MPP frame has an invalid DMA-BUF"));
    }
    // SAFETY: the buffer and its fd remain owned by this frame until
    // the borrow ends.
    let fd = unsafe { BorrowedFd::borrow_raw(fd) };
    Ok(Some(Plane::new(fd, self.info.stride, self.info.rows)))
  }

  pub fn pool_index(&self) -> Option<usize> {
    let buffer = unsafe { mpp_frame_get_buffer(self.handle.as_ptr()) };
    if buffer.is_null() {
      return None;
    }
    let index = unsafe {
      mpp_buffer_get_index_with_caller(
        buffer,
        c"v4l2-stateful-rkmpp".as_ptr(),
      )
    };
    usize::try_from(index).ok()
  }
}
impl Drop for Frame {
  fn drop(&mut self) {
    let mut handle = self.handle.as_ptr();
    unsafe {
      mpp_frame_deinit(&mut handle);
    }
  }
}
