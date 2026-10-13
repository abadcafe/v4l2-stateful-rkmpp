//! module: mpp::group; responsibility: own a decoder output allocation
//! group. operations: new, commit, handle, retire, in_use; entities:
//! Group; tests: group_tests, tests/service/test_resources.
use std::ffi::{c_char, c_void};
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;

use crate::dma::Buffer;
#[repr(C)]
struct BufferInfo {
  kind: u32,
  size: usize,
  ptr: *mut c_void,
  handle: *mut c_void,
  fd: i32,
  index: i32,
}
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_buffer_group_get(
    group: *mut *mut c_void,
    kind: u32,
    mode: u32,
    tag: *const c_char,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_group_put(group: *mut c_void) -> i32;
  fn mpp_buffer_group_clear(group: *mut c_void) -> i32;
  fn mpp_buffer_group_usage(group: *mut c_void) -> usize;
  fn mpp_buffer_group_limit_config(
    group: *mut c_void,
    size: usize,
    count: i32,
  ) -> i32;
  fn mpp_buffer_import_with_tag(
    group: *mut c_void,
    info: *mut BufferInfo,
    buffer: *mut *mut c_void,
    tag: *const c_char,
    caller: *const c_char,
  ) -> i32;
}
pub(super) struct Group {
  handle: NonNull<c_void>,
  buffers: Vec<Buffer>,
}
impl Group {
  pub(super) fn new(ten_bit: bool) -> io::Result<Self> {
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
      mpp_buffer_group_get(
        &mut raw,
        3,
        u32::from(!ten_bit),
        c"v4l2-stateful-rkmpp".as_ptr(),
        c"group_new".as_ptr(),
      )
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP buffer group: {status}"
      )));
    }
    let group = Self {
      handle: NonNull::new(raw)
        .ok_or_else(|| io::Error::other("null MPP group"))?,
      buffers: Vec::new(),
    };
    if ten_bit
      && unsafe { mpp_buffer_group_limit_config(raw, 0, 32) } != 0
    {
      return Err(io::Error::other("MPP buffer group limit"));
    }
    Ok(group)
  }

  pub(super) fn commit(
    &mut self,
    buffers: &[Buffer],
  ) -> io::Result<()> {
    for (index, buffer) in buffers.iter().enumerate() {
      let mut info = BufferInfo {
        kind: 3,
        size: buffer.size(),
        ptr: std::ptr::null_mut(),
        handle: std::ptr::null_mut(),
        fd: buffer.as_fd().as_raw_fd(),
        index: index as i32,
      };
      let status = unsafe {
        mpp_buffer_import_with_tag(
          self.handle.as_ptr(),
          &mut info,
          std::ptr::null_mut(),
          c"v4l2-stateful-rkmpp".as_ptr(),
          c"capture_import".as_ptr(),
        )
      };
      if status != 0 {
        return Err(io::Error::other(format!(
          "MPP capture import: {status}"
        )));
      }
      self.buffers.push(buffer.clone());
    }
    Ok(())
  }

  pub(super) fn handle(&self) -> *mut c_void {
    self.handle.as_ptr()
  }

  pub(super) fn retire(&mut self) -> io::Result<()> {
    // Clear frees unused allocations and marks live decoder/display
    // references for release, without forcing their refcounts or
    // invalidating their fds.
    let status =
      unsafe { mpp_buffer_group_clear(self.handle.as_ptr()) };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP buffer group clear: {status}"
      )));
    }
    Ok(())
  }

  pub(super) fn in_use(&self) -> bool {
    unsafe { mpp_buffer_group_usage(self.handle.as_ptr()) != 0 }
  }
}
impl Drop for Group {
  fn drop(&mut self) {
    unsafe {
      // Outstanding SDK references must never become reusable orphan
      // buffers: they are freed when the last real consumer releases
      // them.
      mpp_buffer_group_clear(self.handle.as_ptr());
      mpp_buffer_group_put(self.handle.as_ptr());
    }
  }
}
