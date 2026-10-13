//! module: mpp::input; responsibility: retain persistent input mappings
//! for SDK packets. operations: new, pointer, size, begin_read;
//! entities: InputMap; tests: input_tests, packet_tests.
use std::ffi::c_void;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;

use crate::dma::{Buffer, CpuAccess};
pub(super) struct InputMap {
  buffer: Buffer,
  ptr: NonNull<u8>,
}
impl InputMap {
  pub(super) fn new(buffer: Buffer) -> io::Result<Self> {
    let protection = libc::PROT_READ;
    // SAFETY: owned fd and validated size remain alive until munmap.
    let raw = unsafe {
      libc::mmap(
        std::ptr::null_mut(),
        buffer.size(),
        protection,
        libc::MAP_SHARED,
        buffer.as_fd().as_raw_fd(),
        0,
      )
    };
    if raw == libc::MAP_FAILED {
      return Err(io::Error::last_os_error());
    }
    let Some(ptr) = NonNull::new(raw.cast()) else {
      unsafe {
        libc::munmap(raw, buffer.size());
      }
      return Err(io::Error::other("null input mapping"));
    };
    Ok(Self { buffer, ptr })
  }

  pub(super) fn pointer(&self) -> *mut c_void {
    self.ptr.as_ptr().cast()
  }

  pub(super) fn size(&self) -> usize {
    self.buffer.size()
  }

  pub(super) fn begin_read(&self) -> io::Result<CpuAccess> {
    self.buffer.begin_read()
  }
}
impl Drop for InputMap {
  fn drop(&mut self) {
    unsafe {
      libc::munmap(self.ptr.as_ptr().cast(), self.buffer.size());
    }
  }
}
