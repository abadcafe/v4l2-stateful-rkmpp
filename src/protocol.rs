//! module: protocol; responsibility: define the published bridge wire
//! contract. operations: batch validation; entities: message, batch,
//! pool and event/reply enums. tests: protocol_tests; end-to-end
//! tests/kernel.
use std::io;
pub const VERSION: u32 = 7;
pub const MAX_BATCH: usize = 16;
pub mod command;
#[cfg(test)]
mod command_tests;
pub mod registration;
#[cfg(test)]
mod registration_tests;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
  Packet = 1,
  Capture,
  Recycle,
  Drain,
  Reset,
  Close,
  Resume,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
  Layout = 32,
  InputDone,
  Frame,
  Eos,
  Stopped,
  Failed,
  Draining,
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct Message {
  pub version: u32,
  pub kind: u32,
  pub epoch: u64,
  pub index: u32,
  pub flags: u32,
  pub timestamp: u64,
  pub bytesused: u32,
  pub codec: u32,
  pub width: u32,
  pub height: u32,
  pub stride: u32,
  pub rows: u32,
  pub size: u32,
  pub format: u32,
  pub count: u32,
  pub generation: u32,
  pub colorspace: u32,
  pub xfer_func: u32,
  pub ycbcr_enc: u32,
  pub quantization: u32,
  pub pool_id: u32,
  pub data_offset: u32,
  pub crop_left: u32,
  pub crop_top: u32,
  pub coded_width: u32,
  pub coded_height: u32,
  pub field: u32,
  pub reserved: [u32; 1],
}
impl Message {
  pub const CAPTURE_REUSE: u32 = 1;
  pub const FRAME_ERROR: u32 = 2;
  pub const RESET_ABORT_DRAIN: u32 = 1 << 17;
  pub const RESET_CAPTURE_ONLY: u32 = 1 << 16;
}
#[repr(C)]
#[derive(Default)]
pub struct Batch {
  pub version: u32,
  pub reply_count: u32,
  pub event_count: u32,
  pub reserved: u32,
  pub replies: [Message; MAX_BATCH],
  pub events: [Message; MAX_BATCH],
  pub results: [i32; MAX_BATCH],
}
impl Batch {
  pub(crate) const IOCTL: libc::c_ulong = (3 << 30)
    | ((std::mem::size_of::<Self>() as libc::c_ulong) << 16)
    | (b'R' as libc::c_ulong) << 8
    | 0x84;

  pub fn validate(&self) -> io::Result<()> {
    if self.version != VERSION
      || self.reply_count as usize > MAX_BATCH
      || self.event_count as usize > MAX_BATCH
      || self.reserved != 0
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid exchange header",
      ));
    }
    Ok(())
  }
}
