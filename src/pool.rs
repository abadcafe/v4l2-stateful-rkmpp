//! module: pool; responsibility: transfer ownership of a validated
//! buffer registration. operations: new, into_parts; entities: Pool,
//! Direction; tests: pool_tests.
use std::io;

use dma_buf::Buffer;

use crate::protocol::registration::MAX_BUFFERS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Direction {
  Output = 10,
  Capture = 9,
}

pub struct Pool {
  direction: Direction,
  id: u32,
  buffers: Vec<Buffer>,
}
impl Pool {
  pub fn new(
    direction: Direction,
    id: u32,
    buffers: Vec<Buffer>,
  ) -> io::Result<Self> {
    if id == 0 || buffers.is_empty() || buffers.len() > MAX_BUFFERS {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid buffer registration",
      ));
    }
    if buffers.iter().enumerate().any(|(index, buffer)| {
      buffers[..index].iter().any(|other| buffer.same_storage(other))
    }) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "buffer registration contains aliased storage",
      ));
    }
    Ok(Self { direction, id, buffers })
  }

  pub fn into_parts(self) -> (Direction, u32, Vec<Buffer>) {
    (self.direction, self.id, self.buffers)
  }
}
