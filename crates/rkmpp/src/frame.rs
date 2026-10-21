//! module: rkmpp::frame; responsibility: own decoded frame leases and
//! borrow their images. operations: from_raw, info, plane, pool_index;
//! entities: Frame, FrameInfo, RawFrame, FrameBuffer, Storage; tests:
//! frame_tests, tests/service.
use std::ffi::{c_char, c_void};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::ptr::NonNull;

use dma_buf::image::{Plane, PlaneCache};
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
  storage: Storage,
  info: FrameInfo,
}
#[derive(Debug)]
struct RawFrame(NonNull<c_void>);
#[derive(Debug)]
struct FrameBuffer {
  frame: RawFrame,
  fd: i32,
}
impl AsFd for FrameBuffer {
  fn as_fd(&self) -> BorrowedFd<'_> {
    // SAFETY: construction validates fd; this owner retains the
    // immutable MppFrame and its buffer until every borrowed plane
    // is released.
    unsafe { BorrowedFd::borrow_raw(self.fd) }
  }
}
#[derive(Debug)]
enum Storage {
  Empty(RawFrame),
  Invalid(RawFrame),
  Image(PlaneCache<FrameBuffer>),
}
impl Storage {
  fn handle(&self) -> *mut c_void {
    match self {
      Self::Empty(frame) | Self::Invalid(frame) => frame.0.as_ptr(),
      Self::Image(image) => image.source().frame.0.as_ptr(),
    }
  }
}
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
  pub width: u32,
  pub height: u32,
  pub stride: u32,
  pub rows: u32,
  pub size: usize,
  pub format: u32,
  pub pts: i64,
  pub dts: i64,
  pub mode: u32,
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
    let frame = RawFrame(NonNull::new(handle)?);
    // SAFETY: the transferred frame is initialized and exclusively
    // owned.
    let info = unsafe { Self::describe(handle) };
    let buffer = unsafe { mpp_frame_get_buffer(handle) };
    let storage = if buffer.is_null() {
      Storage::Empty(frame)
    } else {
      let fd = unsafe {
        mpp_buffer_get_fd_with_caller(
          buffer,
          c"v4l2-stateful-rkmpp".as_ptr(),
        )
      };
      if fd < 0 {
        Storage::Invalid(frame)
      } else {
        Storage::Image(PlaneCache::new(FrameBuffer { frame, fd }))
      }
    };
    Some(Self { storage, info })
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
      pts: unsafe { mpp_frame_get_pts(frame) },
      dts: unsafe { mpp_frame_get_dts(frame) },
      mode: unsafe { mpp_frame_get_mode(frame) },
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
    match &self.storage {
      Storage::Empty(_) => Ok(None),
      Storage::Invalid(_) => {
        Err(io::Error::other("MPP frame has an invalid DMA-BUF"))
      }
      Storage::Image(image) => {
        image.plane(self.info.stride, self.info.rows).map(Some)
      }
    }
  }

  pub fn pool_index(&self) -> Option<usize> {
    let buffer = unsafe { mpp_frame_get_buffer(self.storage.handle()) };
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
impl Drop for RawFrame {
  fn drop(&mut self) {
    let mut handle = self.0.as_ptr();
    unsafe {
      mpp_frame_deinit(&mut handle);
    }
  }
}
