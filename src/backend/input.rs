//! module: backend::input; responsibility: own registered, persistently
//! mapped input submissions. operations:
//! new, id, prepare, task, bytes, complete;
//! entities: InputPool. tests: input_tests, tests/service and
//! tests/service/test_resources.
use std::io;

use dma_buf::memory::Buffer;
use rkmpp::{Input, InputAccess, Packet, Payload};

use crate::timestamp::Source;
struct Slot {
  input: Input,
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
        Ok(Slot { input: Input::new(buffer)?, access: None })
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
    offset: usize,
    length: usize,
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
    slot.access = Some(slot.input.prepare(offset, length)?);
    Ok(())
  }

  pub(super) fn task(
    &self,
    index: usize,
    payload: Payload,
    identity: u64,
    source: Source,
  ) -> io::Result<Packet> {
    self
      .slots
      .get(index)
      .and_then(|s| s.access.as_ref())
      .ok_or_else(|| io::Error::other("input has no read lease"))?
      .packet(payload, identity, source as i64)
  }

  pub(super) fn bytes(&self, index: usize) -> io::Result<&[u8]> {
    self
      .slots
      .get(index)
      .and_then(|s| s.access.as_ref())
      .map(InputAccess::bytes)
      .ok_or_else(|| io::Error::other("input has no read lease"))
  }

  pub(super) fn complete(&mut self, index: usize) -> io::Result<()> {
    let slot = self.slots.get_mut(index).ok_or_else(|| {
      io::Error::new(io::ErrorKind::InvalidInput, "unknown input index")
    })?;
    slot
      .access
      .take()
      .ok_or_else(|| io::Error::other("input has no CPU read lease"))?
      .finish()
  }
}
