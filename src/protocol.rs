//! module: protocol; responsibility: define the published bridge wire
//! contract. operations: batch validation; entities: message, batch,
//! pool and event/reply enums. tests: protocol_tests; end-to-end
//! tests/kernel.
use std::io;
pub const VERSION: u32 = wire::RB_VERSION;
pub const MAX_BATCH: usize = wire::RB_BATCH_MAX as usize;
pub mod command;
#[cfg(test)]
mod command_tests;
pub mod registration;
#[cfg(test)]
mod registration_tests;

use rkmpp_bridge_uapi::wire;
pub use wire::{Batch, Event, Message, Reply};
pub(crate) const ACCEPT_IOCTL: libc::c_ulong = wire::RB_IOCTL_ACCEPT;
pub(crate) const MIN_CAPTURE: u32 = wire::RB_MIN_CAPTURE;
pub fn validate(batch: &Batch) -> io::Result<()> {
  if batch.version != VERSION
    || batch.reply_count as usize > MAX_BATCH
    || batch.event_count as usize > MAX_BATCH
    || batch.reserved != 0
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid exchange header",
    ));
  }
  Ok(())
}
