//! module: backend::capture; responsibility: own output slots and
//! retained display frame leases. operations: new, id, len, next,
//! recycle, hold, image; entities: CapturePool. tests: capture_tests,
//! tests/service pixel/source-change and tests/service/test_resources.
use std::io;

use crate::dma::Buffer;
use crate::image::Image;
use crate::mpp::Frame;
struct Slot {
  held: Option<Frame>,
  available: bool,
  buffer: Buffer,
}
#[derive(Default)]
pub(super) struct CapturePool {
  id: u32,
  slots: Vec<Slot>,
}
impl CapturePool {
  pub(super) fn new(id: u32, buffers: Vec<Buffer>) -> Self {
    Self {
      id,
      slots: buffers
        .into_iter()
        .map(|buffer| Slot { held: None, available: false, buffer })
        .collect(),
    }
  }

  pub(super) fn id(&self) -> u32 {
    self.id
  }

  pub(super) fn len(&self) -> usize {
    self.slots.len()
  }

  pub(super) fn next(&self) -> Option<usize> {
    self.slots.iter().position(|s| s.available)
  }

  pub(super) fn recycle(
    &mut self,
    index: usize,
    changing: bool,
  ) -> io::Result<()> {
    let slot = self.slots.get_mut(index).ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown capture index",
      )
    })?;
    slot.held = None;
    if !changing {
      slot.available = true;
    }
    Ok(())
  }

  pub(super) fn hold(
    &mut self,
    index: usize,
    frame: Option<Frame>,
  ) -> io::Result<()> {
    let slot = self.slots.get_mut(index).ok_or_else(|| {
      io::Error::other("invalid decoded capture index")
    })?;
    if !slot.available {
      return Err(io::Error::other(
        "decoded capture slot is not queued",
      ));
    }
    slot.available = false;
    slot.held = frame;
    Ok(())
  }

  pub(super) fn image(
    &self,
    index: usize,
    stride: u32,
    rows: u32,
  ) -> io::Result<Image> {
    let slot = self.slots.get(index).ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown capture index",
      )
    })?;
    Ok(Image::new(slot.buffer.clone(), stride, rows))
  }
}
