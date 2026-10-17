//! module: mpp::packet; responsibility: own reusable packets over
//! persistent input maps. operations: input, owned, prepare,
//! InputAccess finish; entities: Packet, InputAccess. tests:
//! packet_tests, mpp_tests, V4L2 input ownership.
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;
use std::rc::Rc;

use super::input::InputMap;
use crate::dma::{Buffer, CpuAccess};
use crate::timestamp::Source;
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_packet_init(
    packet: *mut *mut c_void,
    data: *mut c_void,
    size: usize,
  ) -> i32;
  fn mpp_packet_deinit(packet: *mut *mut c_void) -> i32;
  fn mpp_packet_set_pos(packet: *mut c_void, position: *mut c_void);
  fn mpp_packet_set_length(packet: *mut c_void, length: usize);
  fn mpp_packet_get_length(packet: *mut c_void) -> usize;
  fn mpp_packet_set_eos(packet: *mut c_void);
  fn mpp_packet_set_dts(packet: *mut c_void, dts: i64);
  fn mpp_packet_set_pts(packet: *mut c_void, pts: i64);
}
pub struct Packet {
  handle: NonNull<c_void>,
  input: Option<Rc<InputMap>>,
  _owned: Option<Vec<u8>>,
  _lease: Option<Rc<CpuAccess>>,
}
pub struct InputAccess {
  access: Rc<CpuAccess>,
  input: Rc<InputMap>,
  offset: usize,
  length: usize,
}
impl InputAccess {
  pub fn bytes(&self) -> &[u8] {
    // The lease excludes CPU writers and retains its mapping for this
    // borrow.
    unsafe {
      std::slice::from_raw_parts(
        self.input.pointer().cast::<u8>().add(self.offset),
        self.length,
      )
    }
  }

  /// Borrow a checked subrange for one synchronous coded-frame task.
  /// The input owner retains this CPU read lease until all tasks
  /// complete.
  pub fn packet(
    &self,
    range: std::ops::Range<usize>,
    identity: u64,
    source: Source,
  ) -> io::Result<Packet> {
    if range.start >= range.end || range.end > self.length {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "task range outside input lease",
      ));
    }
    let mut handle = std::ptr::null_mut();
    let pointer = unsafe {
      self.input.pointer().cast::<u8>().add(self.offset + range.start)
    };
    let status = unsafe {
      mpp_packet_init(&mut handle, pointer.cast(), range.len())
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP task packet: {status}"
      )));
    }
    let handle = NonNull::new(handle)
      .ok_or_else(|| io::Error::other("null MPP task packet"))?;
    unsafe {
      mpp_packet_set_pts(handle.as_ptr(), identity as i64);
      mpp_packet_set_dts(handle.as_ptr(), source as i64);
    }
    Ok(Packet {
      handle,
      input: Some(self.input.clone()),
      _owned: None,
      _lease: Some(self.access.clone()),
    })
  }

  pub fn finish(self) -> io::Result<()> {
    Rc::try_unwrap(self.access)
      .map_err(|_| {
        io::Error::other("input still has live coded-frame tasks")
      })?
      .finish()
  }
}
impl Packet {
  pub(super) fn end() -> io::Result<Self> {
    let packet = Self::owned(vec![0], 0, Source::End)?;
    unsafe {
      mpp_packet_set_length(packet.handle.as_ptr(), 0);
      mpp_packet_set_eos(packet.handle.as_ptr());
    }
    Ok(packet)
  }

  pub fn input(buffer: Buffer) -> io::Result<Self> {
    let input = Rc::new(InputMap::new(buffer)?);
    let mut handle = std::ptr::null_mut();
    // SAFETY: packet borrows this owned map, which is released only
    // after packet deinit.
    let status = unsafe {
      mpp_packet_init(&mut handle, input.pointer(), input.size())
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP packet init: {status}"
      )));
    }
    Ok(Self {
      handle: NonNull::new(handle)
        .ok_or_else(|| io::Error::other("null MPP packet"))?,
      input: Some(input),
      _owned: None,
      _lease: None,
    })
  }

  pub fn owned(
    mut bytes: Vec<u8>,
    timestamp: u64,
    source: Source,
  ) -> io::Result<Self> {
    if bytes.is_empty()
      || bytes.len() > crate::submission::MAX_PACKET_SIZE
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid access unit size",
      ));
    }
    let length = bytes.len();
    bytes.resize(length + 64, 0);
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
      mpp_packet_init(&mut handle, bytes.as_mut_ptr().cast(), length)
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP packet init: {status}"
      )));
    }
    let handle = NonNull::new(handle)
      .ok_or_else(|| io::Error::other("null MPP packet"))?;
    unsafe {
      mpp_packet_set_pts(handle.as_ptr(), timestamp as i64);
      // No decode clock is supplied by V4L2. Use MPP's preserved DTS
      // metadata channel for the timestamp origin; PTS retains frame
      // identity.
      mpp_packet_set_dts(handle.as_ptr(), source as i64);
    }
    Ok(Self { handle, input: None, _owned: Some(bytes), _lease: None })
  }

  pub fn prepare(
    &mut self,
    offset: usize,
    length: usize,
    timestamp: u64,
    source: Source,
  ) -> io::Result<InputAccess> {
    let input = self.input.as_ref().ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "owned packet has no registered input map",
      )
    })?;
    if length == 0
      || offset.checked_add(length).is_none_or(|end| end > input.size())
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "packet length outside registered buffer",
      ));
    }
    let access = input.begin_read()?;
    unsafe {
      mpp_packet_set_pos(
        self.handle.as_ptr(),
        input.pointer().cast::<u8>().add(offset).cast(),
      );
      mpp_packet_set_length(self.handle.as_ptr(), length);
      mpp_packet_set_pts(self.handle.as_ptr(), timestamp as i64);
      mpp_packet_set_dts(self.handle.as_ptr(), source as i64);
    }
    Ok(InputAccess {
      access: Rc::new(access),
      input: input.clone(),
      offset,
      length,
    })
  }

  pub(super) fn consumed(&self) -> bool {
    unsafe { mpp_packet_get_length(self.handle.as_ptr()) == 0 }
  }

  pub(super) fn handle(&self) -> *mut c_void {
    self.handle.as_ptr()
  }
}
impl Drop for Packet {
  fn drop(&mut self) {
    let mut handle = self.handle.as_ptr();
    unsafe {
      mpp_packet_deinit(&mut handle);
    }
  }
}
