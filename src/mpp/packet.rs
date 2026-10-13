//! module: mpp::packet; responsibility: own reusable packets over
//! persistent input maps. operations: input, eos, prepare, InputAccess
//! finish; entities: Packet, InputAccess. tests: packet_tests,
//! mpp_tests, V4L2 input ownership.
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

use super::input::InputMap;
use crate::dma::{Buffer, CpuAccess};
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_packet_init(
    packet: *mut *mut c_void,
    data: *mut c_void,
    size: usize,
  ) -> i32;
  fn mpp_packet_new(packet: *mut *mut c_void) -> i32;
  fn mpp_packet_deinit(packet: *mut *mut c_void) -> i32;
  fn mpp_packet_set_eos(packet: *mut c_void) -> i32;
  fn mpp_packet_set_pos(packet: *mut c_void, position: *mut c_void);
  fn mpp_packet_set_length(packet: *mut c_void, length: usize);
  fn mpp_packet_set_pts(packet: *mut c_void, pts: i64);
}
pub struct Packet {
  handle: NonNull<c_void>,
  input: Option<InputMap>,
}
pub struct InputAccess(CpuAccess);
impl InputAccess {
  pub fn finish(self) -> io::Result<()> {
    self.0.finish()
  }
}
impl Packet {
  pub fn input(buffer: Buffer) -> io::Result<Self> {
    let input = InputMap::new(buffer)?;
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
    })
  }

  pub fn eos() -> io::Result<Self> {
    let mut handle = std::ptr::null_mut();
    let status = unsafe { mpp_packet_new(&mut handle) };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP EOS packet: {status}"
      )));
    }
    let packet = Self {
      handle: NonNull::new(handle)
        .ok_or_else(|| io::Error::other("null MPP packet"))?,
      input: None,
    };
    if unsafe { mpp_packet_set_eos(packet.handle.as_ptr()) } != 0 {
      return Err(io::Error::other("set MPP EOS"));
    }
    Ok(packet)
  }

  pub fn prepare(
    &mut self,
    length: usize,
    timestamp: u64,
  ) -> io::Result<InputAccess> {
    let input = self.input.as_ref().ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "EOS packet has no input map",
      )
    })?;
    if length == 0 || length > input.size() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "packet length outside registered buffer",
      ));
    }
    let access = input.begin_read()?;
    unsafe {
      mpp_packet_set_pos(self.handle.as_ptr(), input.pointer());
      mpp_packet_set_length(self.handle.as_ptr(), length);
      mpp_packet_set_pts(self.handle.as_ptr(), timestamp as i64);
    }
    Ok(InputAccess(access))
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
