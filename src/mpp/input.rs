//! module: mpp::input; responsibility: retain persistent input mappings
//! for SDK packets. operations: new, read, bytes, finish;
//! entities: InputMap, ReadAccess; tests: input_tests, packet_tests.
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;
use std::rc::Rc;

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

  pub(super) fn read(
    self: &Rc<Self>,
    offset: usize,
    length: usize,
  ) -> io::Result<ReadAccess> {
    if length == 0
      || offset
        .checked_add(length)
        .is_none_or(|end| end > self.buffer.size())
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "packet length outside registered buffer",
      ));
    }
    Ok(ReadAccess {
      map: self.clone(),
      lease: self.buffer.begin_read()?,
      offset,
      length,
    })
  }
}
impl Drop for InputMap {
  fn drop(&mut self) {
    unsafe {
      libc::munmap(self.ptr.as_ptr().cast(), self.buffer.size());
    }
  }
}

// The range, mapping lifetime, and CPU exclusion are one ownership
// unit. A consumer cannot construct a slice without holding all three.
pub(super) struct ReadAccess {
  map: Rc<InputMap>,
  lease: CpuAccess,
  offset: usize,
  length: usize,
}
impl ReadAccess {
  pub(super) fn bytes(&self) -> &[u8] {
    // SAFETY: read checked this nonempty range against the allocation;
    // map retains it and lease excludes competing CPU mappings for the
    // entire borrow. The mapping is read-only.
    unsafe {
      std::slice::from_raw_parts(
        self.map.ptr.as_ptr().add(self.offset),
        self.length,
      )
    }
  }

  pub(super) fn finish(self) -> io::Result<()> {
    self.lease.finish()
  }
}
