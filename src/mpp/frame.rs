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
  metadata: std::sync::OnceLock<crate::dma::system::Metadata>,
}
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
  /// # Safety
  /// A non-null handle transfers one initialized MppFrame reference;
  /// no other owner may mutate or deinitialize it after this call.
  pub(super) unsafe fn from_raw(handle: *mut c_void) -> Option<Self> {
    NonNull::new(handle).map(|handle| Self {
      info: unsafe { Self::describe(handle.as_ptr()) },
      metadata: std::sync::OnceLock::new(),
      handle,
    })
  }

  pub fn info(&self) -> FrameInfo {
    self.info
  }

  unsafe fn describe(frame: *mut c_void) -> FrameInfo {
    // SAFETY: this owner holds the frame for every getter, with no
    // simultaneous mutation.
    FrameInfo {
      width: unsafe { mpp_frame_get_width(frame) },
      height: unsafe { mpp_frame_get_height(frame) },
      stride: unsafe { mpp_frame_get_hor_stride(frame) },
      rows: unsafe { mpp_frame_get_ver_stride(frame) },
      size: unsafe { mpp_frame_get_buf_size(frame) },
      format: unsafe { mpp_frame_get_fmt(frame) } & !0x0c000000,
      timestamp: unsafe { mpp_frame_get_pts(frame) } as u64,
      timestamp_source: unsafe { mpp_frame_get_dts(frame) } as u32,
      field: match unsafe { mpp_frame_get_mode(frame) } & 15 {
        1 => 3,
        2 => 4,
        7 => 1,
        11 => 2,
        3 | 15 => 5,
        _ => 0,
      },
      eos: unsafe { mpp_frame_get_eos(frame) } != 0,
      info_change: unsafe { mpp_frame_get_info_change(frame) } != 0,
      failed: unsafe { mpp_frame_get_errinfo(frame) } != 0
        || unsafe { mpp_frame_get_discard(frame) } != 0,
      primaries: unsafe { mpp_frame_get_color_primaries(frame) },
      matrix: unsafe { mpp_frame_get_colorspace(frame) },
      transfer: unsafe { mpp_frame_get_color_trc(frame) },
      range: unsafe { mpp_frame_get_color_range(frame) },
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
    Ok(Some(Plane::registered(
      fd,
      self.info.stride,
      self.info.rows,
      &self.metadata,
    )))
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
