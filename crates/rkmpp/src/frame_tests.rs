//! module: rkmpp::frame_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! BufferInfo; tests:
//! test_from_raw_null_handle_has_no_frame
//! test_info_owned_sdk_frame_preserves_extent_timestamp_and_flags
//! test_info_preserves_color_metadata_and_masks_only_frame_transport_flags
//! test_plane_borrows_sdk_buffer_and_preserves_capture_pool_index
use std::ffi::{c_char, c_void};
use std::os::fd::{AsFd, AsRawFd};

use dma_buf::Buffer;

use super::frame::Frame;

#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_frame_init(frame: *mut *mut c_void) -> i32;
  fn mpp_frame_set_width(frame: *mut c_void, value: u32);
  fn mpp_frame_set_height(frame: *mut c_void, value: u32);
  fn mpp_frame_set_hor_stride(frame: *mut c_void, value: u32);
  fn mpp_frame_set_ver_stride(frame: *mut c_void, value: u32);
  fn mpp_frame_set_pts(frame: *mut c_void, value: i64);
  fn mpp_frame_set_dts(frame: *mut c_void, value: i64);
  fn mpp_frame_set_mode(frame: *mut c_void, value: u32);
  fn mpp_frame_set_eos(frame: *mut c_void, value: u32);
  fn mpp_frame_set_errinfo(frame: *mut c_void, value: u32);
  fn mpp_frame_set_info_change(frame: *mut c_void, value: u32);
  fn mpp_frame_set_fmt(frame: *mut c_void, value: u32);
  fn mpp_frame_set_discard(frame: *mut c_void, value: u32);
  fn mpp_frame_set_color_primaries(frame: *mut c_void, value: u32);
  fn mpp_frame_set_colorspace(frame: *mut c_void, value: u32);
  fn mpp_frame_set_color_trc(frame: *mut c_void, value: u32);
  fn mpp_frame_set_color_range(frame: *mut c_void, value: u32);
  fn mpp_frame_set_buffer(frame: *mut c_void, buffer: *mut c_void);
  fn mpp_buffer_import_with_tag(
    group: *mut c_void,
    info: *mut BufferInfo,
    buffer: *mut *mut c_void,
    tag: *const c_char,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_put_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
}

#[test]
fn test_from_raw_null_handle_has_no_frame() {
  assert!(unsafe { Frame::from_raw(std::ptr::null_mut()) }.is_none());
}

#[test]
fn test_info_preserves_signed_clocks_and_raw_sdk_field_flags() {
  let mut raw = std::ptr::null_mut();
  assert_eq!(unsafe { mpp_frame_init(&mut raw) }, 0);
  // SAFETY: the SDK initialized this uniquely owned frame.
  unsafe {
    mpp_frame_set_pts(raw, i64::MIN);
    mpp_frame_set_dts(raw, -17);
    mpp_frame_set_mode(raw, 0x207);
  }
  // SAFETY: ownership of the initialized frame transfers to Frame.
  let frame = unsafe { Frame::from_raw(raw) }.unwrap();
  let info = frame.info();
  assert_eq!((info.pts, info.dts, info.mode), (i64::MIN, -17, 0x207));
}

#[test]
fn test_info_owned_sdk_frame_preserves_extent_timestamp_and_flags() {
  let mut raw = std::ptr::null_mut();
  assert_eq!(unsafe { mpp_frame_init(&mut raw) }, 0);
  unsafe {
    mpp_frame_set_width(raw, 256);
    mpp_frame_set_height(raw, 128);
    mpp_frame_set_hor_stride(raw, 320);
    mpp_frame_set_ver_stride(raw, 144);
    mpp_frame_set_pts(raw, 123456);
    mpp_frame_set_eos(raw, 1);
    mpp_frame_set_errinfo(raw, 1);
    mpp_frame_set_info_change(raw, 1);
  }
  let frame = unsafe { Frame::from_raw(raw) }.unwrap();
  let info = frame.info();
  assert_eq!(
    (info.width, info.height, info.stride, info.rows, info.pts),
    (256, 128, 320, 144, 123456)
  );
  assert!(info.eos && info.failed && info.info_change);
  assert!(frame.plane().unwrap().is_none());
  assert_eq!(frame.pool_index(), None);
}

#[test]
fn test_info_preserves_color_metadata_and_masks_only_frame_transport_flags()
 {
  for (primaries, matrix, transfer, range) in
    [(1, 1, 1, 1), (9, 9, 16, 1), (9, 9, 18, 2)]
  {
    let mut raw = std::ptr::null_mut();
    assert_eq!(unsafe { mpp_frame_init(&mut raw) }, 0);
    unsafe {
      mpp_frame_set_fmt(raw, 0x0c000001);
      mpp_frame_set_color_primaries(raw, primaries);
      mpp_frame_set_colorspace(raw, matrix);
      mpp_frame_set_color_trc(raw, transfer);
      mpp_frame_set_color_range(raw, range);
      mpp_frame_set_discard(raw, 1);
    }
    let frame = unsafe { Frame::from_raw(raw) }.unwrap();
    let info = frame.info();
    assert_eq!(
      (
        info.primaries,
        info.matrix,
        info.transfer,
        info.range,
        info.format
      ),
      (primaries, matrix, transfer, range, 1)
    );
    assert!(info.failed);
  }
}

#[repr(C)]
struct BufferInfo {
  kind: u32,
  size: usize,
  ptr: *mut c_void,
  handle: *mut c_void,
  fd: i32,
  index: i32,
}
#[test]
fn test_plane_borrows_sdk_buffer_and_preserves_capture_pool_index() {
  let buffer = Buffer::allocate(4096).unwrap();
  let mut info = BufferInfo {
    kind: 2,
    size: buffer.size(),
    ptr: std::ptr::null_mut(),
    handle: std::ptr::null_mut(),
    fd: buffer.as_fd().as_raw_fd(),
    index: 7,
  };
  let mut sdk = std::ptr::null_mut();
  assert_eq!(
    unsafe {
      mpp_buffer_import_with_tag(
        std::ptr::null_mut(),
        &mut info,
        &mut sdk,
        c"frame_contract".as_ptr(),
        c"test_plane".as_ptr(),
      )
    },
    0
  );
  let mut raw = std::ptr::null_mut();
  assert_eq!(unsafe { mpp_frame_init(&mut raw) }, 0);
  unsafe {
    mpp_frame_set_hor_stride(raw, 64);
    mpp_frame_set_ver_stride(raw, 32);
    mpp_frame_set_buffer(raw, sdk);
  }
  assert_eq!(
    unsafe { mpp_buffer_put_with_caller(sdk, c"test_plane".as_ptr()) },
    0
  );
  let frame = unsafe { Frame::from_raw(raw) }.unwrap();
  let plane = frame.plane().unwrap().unwrap();
  assert_eq!((plane.stride(), plane.rows()), (64, 32));
  assert!(
    unsafe { libc::fcntl(plane.fd().as_raw_fd(), libc::F_GETFD) } >= 0
  );
  assert_eq!(frame.pool_index(), Some(7));
  drop(frame);
  buffer.write().unwrap().finish().unwrap();
}
