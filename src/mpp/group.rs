//! module: mpp::group; responsibility: own a decoder output allocation
//! group. operations: new, commit, handle, retire, in_use; entities:
//! Group; tests: group_tests, tests/service/test_resources.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_void};
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;

use super::reservation::Reservation;
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
  fn mpp_buffer_group_unused(group: *mut c_void) -> i32;
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
  external: bool,
  buffers: Vec<Buffer>,
  capture: BTreeSet<usize>,
  handles: BTreeMap<usize, NonNull<c_void>>,
  private: usize,
  capture_capacity: usize,
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
      capture: BTreeSet::new(),
      handles: BTreeMap::new(),
      private: 0,
      capture_capacity: 0,
      external: !ten_bit,
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
    let indices: BTreeSet<_> =
      buffers.iter().map(|(index, _)| *index).collect();
    if !(1..=32).contains(&capacity)
      || (self.capture_capacity != 0
        && self.capture_capacity != capacity)
      || indices.len() != buffers.len()
      || indices
        .iter()
        .any(|index| *index >= capacity || self.capture.contains(index))
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid capture registration",
      ));
    }
    self.capture_capacity = capacity;
    for (index, buffer) in buffers {
      self.import(*index, buffer.clone())?;
      self.capture.insert(*index);
    }
    Ok(())
  }

  fn import(&mut self, index: usize, buffer: Buffer) -> io::Result<()> {
    let mut info = BufferInfo {
      kind: 3,
      size: buffer.size(),
      ptr: std::ptr::null_mut(),
      handle: std::ptr::null_mut(),
      fd: buffer.as_fd().as_raw_fd(),
      index: i32::try_from(index)
        .map_err(|_| io::Error::other("capture index overflow"))?,
    };
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
      mpp_buffer_import_with_tag(
        self.handle.as_ptr(),
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
    self.handles.insert(index, raw);
    // The group owns the unused buffer after releasing the import
    // reference.
    drop(unsafe { Reservation::imported(raw) });
    self.buffers.push(buffer);
    Ok(())
  }

  /// MPP and normative output scheduling can retain different pictures.
  /// Bound their combined storage to 16 decoder + 15 pending + 1
  /// working surfaces. Allocate only under pressure; ordinary decode
  /// stays zero-copy.
  pub(super) fn ensure_available(&mut self) -> io::Result<bool> {
    if self.available() {
      return Ok(true);
    }
    if !self.external || self.capture_capacity + self.private >= 32 {
      return Ok(false);
    }
    let Some(size) = self.buffers.first().map(Buffer::size) else {
      return Ok(false);
    };
    self.import(32 + self.private, Buffer::allocate(size)?)?;
    self.private += 1;
    Ok(true)
  }

  /// Temporarily retain private surfaces while selecting a real CAPTURE
  /// destination, so a GPU copy never overwrites a decoder reference.
  pub(super) fn reserve_capture(
    &self,
    size: usize,
  ) -> io::Result<Option<Reservation>> {
    let mut private = Vec::new();
    while self.available() {
      let Some(lease) = Reservation::acquire(self.handle(), size)?
      else {
        break;
      };
      if self.capture.contains(&lease.index()?) {
        return Ok(Some(lease));
      }
      private.push(lease);
    }
    Ok(None)
  }

  pub(super) fn pin_capture(&self) -> io::Result<Vec<Reservation>> {
    self
      .capture
      .iter()
      .map(|index| {
        let handle = self.handles.get(index).ok_or_else(|| {
          io::Error::other("retired capture registration")
        })?;
        // SAFETY: the live group retains every registered buffer, even
        // at refcount zero.
        unsafe { Reservation::retain(*handle) }
      })
      .collect()
  }

  pub(super) fn available(&self) -> bool {
    unsafe { mpp_buffer_group_unused(self.handle.as_ptr()) > 0 }
  }

  pub(super) fn external(&self) -> bool {
    self.external
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
    self.handles.clear();
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
