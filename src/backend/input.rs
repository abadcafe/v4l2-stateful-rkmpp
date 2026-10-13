//! module: backend::input; responsibility: own registered, persistently
//! mapped input slots. operations: new, id, prepare, packet, complete;
//! entities: InputPool. tests: input_tests, tests/service and
//! tests/service/test_resources.
use std::io;

use crate::dma::Buffer;
use crate::mpp::{InputAccess, Packet};
struct Slot {
  packet: Packet,
  access: Option<InputAccess>,
}
impl Drop for Slot {
  fn drop(&mut self) {
    self.access = None;
  }
}
#[derive(Default)]
pub(super) struct InputPool {
  id: u32,
  slots: Vec<Slot>,
}
impl InputPool {
  pub(super) fn new(id: u32, buffers: Vec<Buffer>) -> io::Result<Self> {
    let slots = buffers
      .into_iter()
      .map(|buffer| {
        if buffer.size() > 16 * 1024 * 1024 {
          return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input buffer exceeds 16 MiB",
          ));
        }
        Ok(Slot { packet: Packet::input(buffer)?, access: None })
      })
      .collect::<io::Result<Vec<_>>>()?;
    Ok(Self { id, slots })
  }

  pub(super) fn id(&self) -> u32 {
    self.id
  }

  pub(super) fn prepare(
    &mut self,
    index: usize,
    length: usize,
    timestamp: u64,
  ) -> io::Result<()> {
    let slot = self.slots.get_mut(index).ok_or_else(|| {
      io::Error::new(io::ErrorKind::InvalidInput, "unknown input index")
    })?;
    if slot.access.is_some() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "input buffer already queued",
      ));
    }
    slot.access = Some(slot.packet.prepare(length, timestamp)?);
    Ok(())
  }

  pub(super) fn packet(&self, index: usize) -> io::Result<&Packet> {
    self.slots.get(index).map(|slot| &slot.packet).ok_or_else(|| {
      io::Error::new(io::ErrorKind::InvalidInput, "invalid input slot")
    })
  }

  pub(super) fn complete(&mut self, index: usize) -> io::Result<()> {
    self
      .slots
      .get_mut(index)
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidInput,
          "unknown input index",
        )
      })?
      .access
      .take()
      .ok_or_else(|| io::Error::other("input has no CPU read lease"))?
      .finish()
  }
}
