//! module: rkmpp::group; responsibility: own a decoder output
//! allocation group. operations: new, commit, handle, retire, in_use,
//! reserve_capture; entities: Group, Allocation, Capture, Registered,
//! BufferInfo; tests: group_tests,
//! tests/service/test_resources.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;

use dma_buf::memory::Buffer;

use super::ffi::{
  MppBufferInfo as BufferInfo, mpp_buffer_get_with_tag,
  mpp_buffer_group_clear, mpp_buffer_group_get,
  mpp_buffer_group_limit_config, mpp_buffer_group_put,
  mpp_buffer_group_unused, mpp_buffer_group_usage,
  mpp_buffer_import_with_tag,
};
use super::reservation::Reservation;
pub(super) struct Group {
  handle: NonNull<c_void>,
  allocation: Allocation,
}
enum Allocation {
  Internal,
  Capture(Capture),
}
struct Capture {
  buffers: BTreeMap<usize, Registered>,
  capacity: usize,
}
struct Registered {
  buffer: Buffer,
  handle: Option<NonNull<c_void>>,
}
fn internal_capture() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidInput,
    "CAPTURE operation requires an external allocation group",
  )
}
impl Group {
  pub(super) fn new(ten_bit: bool) -> io::Result<Self> {
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
      mpp_buffer_group_get(
        &mut raw,
        super::ffi::MPP_BUFFER_TYPE_DRM,
        if ten_bit {
          super::ffi::MPP_BUFFER_INTERNAL
        } else {
          super::ffi::MPP_BUFFER_EXTERNAL
        },
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
      allocation: if ten_bit {
        Allocation::Internal
      } else {
        Allocation::Capture(Capture {
          buffers: BTreeMap::new(),
          capacity: 0,
        })
      },
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
    buffers: &[(usize, Buffer)],
    capacity: usize,
  ) -> io::Result<()> {
    let Allocation::Capture(capture) = &mut self.allocation else {
      return Err(internal_capture());
    };
    let indices: BTreeSet<_> =
      buffers.iter().map(|(index, _)| *index).collect();
    if !(1..=32).contains(&capacity)
      || (capture.capacity != 0 && capture.capacity != capacity)
      || indices.len() != buffers.len()
      || indices.iter().any(|index| {
        *index >= capacity || capture.buffers.contains_key(index)
      })
      || buffers.iter().enumerate().any(|(position, (_, buffer))| {
        capture
          .buffers
          .values()
          .any(|old| buffer.same_storage(&old.buffer))
          || buffers[..position]
            .iter()
            .any(|(_, old)| buffer.same_storage(old))
      })
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid capture registration",
      ));
    }
    capture.capacity = capacity;
    for (index, buffer) in buffers {
      let imported = Self::import(self.handle, *index, buffer.clone())?;
      capture.buffers.insert(*index, imported);
    }
    Ok(())
  }

  fn import(
    group: NonNull<c_void>,
    index: usize,
    buffer: Buffer,
  ) -> io::Result<Registered> {
    let mut info = BufferInfo {
      type_: super::ffi::MPP_BUFFER_TYPE_DRM,
      size: buffer.size(),
      ptr: std::ptr::null_mut(),
      hnd: std::ptr::null_mut(),
      fd: buffer.as_fd().as_raw_fd(),
      index: i32::try_from(index)
        .map_err(|_| io::Error::other("capture index overflow"))?,
    };
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
      mpp_buffer_import_with_tag(
        group.as_ptr(),
        &mut info,
        &mut raw,
        c"v4l2-stateful-rkmpp".as_ptr(),
        c"capture_import".as_ptr(),
      )
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP capture import: {status}"
      )));
    }
    let raw = NonNull::new(raw)
      .ok_or_else(|| io::Error::other("MPP imported a null buffer"))?;
    // The group owns the unused buffer after releasing the import
    // reference.
    drop(unsafe { Reservation::imported(raw) });
    Ok(Registered { buffer, handle: Some(raw) })
  }

  /// Reserve only registered client storage. Exhaustion is
  /// backpressure; allocation policy belongs to CAPTURE negotiation,
  /// not this pool.
  pub(super) fn reserve_capture(
    &self,
    size: usize,
  ) -> io::Result<Option<Reservation>> {
    self.capture()?;
    if !self.available() {
      return Ok(None);
    }
    let mut buffer = std::ptr::null_mut();
    // SAFETY: this owner keeps the group alive throughout acquisition;
    // success transfers one SDK reference to the returned lease.
    let result = unsafe {
      mpp_buffer_get_with_tag(
        self.handle.as_ptr(),
        &mut buffer,
        size,
        c"v4l2-stateful-rkmpp".as_ptr(),
        c"capture_reserve".as_ptr(),
      )
    };
    if !super::status::reservation(result)? {
      return Ok(None);
    }
    let buffer = NonNull::new(buffer).ok_or_else(|| {
      io::Error::other("MPP returned a null reserved buffer")
    })?;
    // SAFETY: successful acquisition above owns exactly one reference.
    Ok(Some(unsafe { Reservation::imported(buffer) }))
  }

  pub(super) fn pin_capture(&self) -> io::Result<Vec<Reservation>> {
    self
      .capture()?
      .buffers
      .values()
      .map(|registered| {
        let handle = registered.handle.ok_or_else(|| {
          io::Error::other("retired capture registration")
        })?;
        // SAFETY: the live group retains every registered buffer, even
        // at refcount zero.
        unsafe { Reservation::retain(handle) }
      })
      .collect()
  }

  pub(super) fn available(&self) -> bool {
    unsafe { mpp_buffer_group_unused(self.handle.as_ptr()) > 0 }
  }

  pub(super) fn external(&self) -> bool {
    matches!(self.allocation, Allocation::Capture(_))
  }

  fn capture(&self) -> io::Result<&Capture> {
    match &self.allocation {
      Allocation::Capture(capture) => Ok(capture),
      Allocation::Internal => Err(internal_capture()),
    }
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
    if let Allocation::Capture(capture) = &mut self.allocation {
      for registered in capture.buffers.values_mut() {
        registered.handle = None;
      }
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
