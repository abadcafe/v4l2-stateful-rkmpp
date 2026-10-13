//! module: transport::registration; responsibility: adopt an owned
//! kernel buffer export atomically. operations: adopt; entities: none;
//! tests: registration_tests.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::dma::Buffer;
use crate::pool::{Direction, Pool as RegisteredPool};
use crate::protocol::{MAX_BUFFERS, Pool, VERSION};

pub(super) fn adopt(
  pool: Pool,
  direction: Direction,
  epoch: u64,
  id: u32,
) -> io::Result<RegisteredPool> {
  // Successful REGISTER_POOL has transferred these unique descriptors
  // to us. Adopt all returned owners before validating so every
  // rejection closes them.
  let descriptors =
    &pool.buffers[..(pool.count as usize).min(MAX_BUFFERS)];
  let mut owners = Vec::with_capacity(descriptors.len());
  let mut invalid = false;
  for descriptor in descriptors {
    if descriptor.fd < 0
      || owners
        .iter()
        .any(|fd: &OwnedFd| fd.as_raw_fd() == descriptor.fd)
    {
      invalid = true;
    } else {
      owners.push(unsafe { OwnedFd::from_raw_fd(descriptor.fd) });
    }
  }
  if invalid
    || pool.version != VERSION
    || pool.kind != direction as u32
    || pool.epoch != epoch
    || pool.pool_id != id
    || pool.count == 0
    || pool.count as usize > MAX_BUFFERS
    || pool.reserved != [0; 2]
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid registered pool response",
    ));
  }
  let buffers = owners
    .into_iter()
    .zip(descriptors)
    .map(|(fd, descriptor)| {
      Buffer::from_owned(fd, descriptor.size as usize)
    })
    .collect::<io::Result<Vec<_>>>()?;
  RegisteredPool::new(direction, id, buffers)
}
