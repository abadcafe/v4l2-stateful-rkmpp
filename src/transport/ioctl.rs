//! module: transport::ioctl; responsibility: complete owned kernel ABI
//! transactions on a blocking execution lane. operations: accept,
//! exchange, register; entities: none; tests: transport_tests and
//! process acceptance.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use rkmpp_bridge_uapi::wire::{
  RB_IOCTL_EXCHANGE, RB_IOCTL_REGISTER_POOL,
};

use super::admission::Admission;
use crate::pool::{Direction, Pool as RegisteredPool};
use crate::protocol::Batch;
pub(super) fn accept(
  lease: crate::descriptor::Lease,
) -> io::Result<Admission<OwnedFd>> {
  let fd = unsafe {
    libc::ioctl(lease.as_raw_fd(), crate::protocol::ACCEPT_IOCTL)
  };
  if fd < 0 {
    return Admission::classify(io::Error::last_os_error());
  }
  // SAFETY: ACCEPT installs and returns a fresh session fd in this
  // process.
  Ok(Admission::Accepted(unsafe { OwnedFd::from_raw_fd(fd) }))
}
pub(super) fn exchange(
  lease: crate::descriptor::Lease,
  batch: &mut Batch,
) -> io::Result<()> {
  crate::protocol::validate(batch)?;
  let submitted = batch.reply_count;
  // SAFETY: Batch matches bridge_uapi.h and remains writable until
  // ioctl completes.
  if unsafe {
    libc::ioctl(lease.as_raw_fd(), RB_IOCTL_EXCHANGE, &mut *batch)
  } < 0
  {
    return Err(io::Error::last_os_error());
  }
  crate::protocol::validate(batch)?;
  // Each submitted reply has exactly one zero-or-negative errno
  // result. Reject malformed results here, before scheduling events
  // or converting a negative status to its positive errno.
  if batch.reply_count != submitted
    || batch.results[..submitted as usize]
      .iter()
      .any(|status| !(-4095..=0).contains(status))
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid exchange response",
    ));
  }
  Ok(())
}

pub(super) fn register(
  lease: crate::descriptor::Lease,
  direction: Direction,
  epoch: u64,
  id: u32,
) -> io::Result<RegisteredPool> {
  let mut pool =
    crate::protocol::registration::request(direction as u32, epoch, id);
  // SAFETY: REGISTER_POOL installs fresh fds only on successful
  // copyout.
  if unsafe {
    libc::ioctl(lease.as_raw_fd(), RB_IOCTL_REGISTER_POOL, &mut pool)
  } < 0
  {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: the successful REGISTER_POOL above transferred unique
  // kernel-exported descriptors into this response. No other Rust
  // owner has been constructed for them.
  unsafe { super::registration::adopt(pool, direction, epoch, id) }
}
