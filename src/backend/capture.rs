//! module: backend::capture; responsibility: enforce CAPTURE
//! write/display ownership. operations: registration/import/recycle,
//! present, prepare_write, reuse; entities: CapturePool, Target,
//! Presentation, Slot, SlotState, Lease; tests: capture_tests. Target
//! borrows the pool exclusively until commit or drop.
use std::io;
use std::os::fd::{AsFd, AsRawFd};

use crate::dma::{Buffer, system};
use crate::image::Image;
use crate::mpp::{Decoder, Frame, Reservation};
enum Lease {
  Decoded(Frame),
  Reserved(Reservation),
  Private,
}
enum SlotState {
  Idle,
  Queued,
  Held(Lease),
}
struct Slot {
  state: SlotState,
  imported: bool,
  buffer: Buffer,
}
#[derive(Default)]
pub(super) struct CapturePool {
  id: u32,
  slots: Vec<Slot>,
}
pub(super) enum Presentation {
  Direct(usize),
  Convert(Frame),
}
pub(super) struct Target<'a> {
  pool: &'a mut CapturePool,
  index: usize,
  image: Image,
  reservation: Option<Reservation>,
}
impl Target<'_> {
  pub(super) fn index(&self) -> usize {
    self.index
  }

  pub(super) fn image(&self) -> Image {
    self.image.clone()
  }

  pub(super) fn commit(self) -> usize {
    self.pool.slots[self.index].state = SlotState::Held(
      self.reservation.map_or(Lease::Private, Lease::Reserved),
    );
    self.index
  }
}
impl CapturePool {
  pub(super) fn new(id: u32, buffers: Vec<Buffer>) -> Self {
    Self {
      id,
      slots: buffers
        .into_iter()
        .map(|buffer| Slot {
          state: SlotState::Idle,
          imported: false,
          buffer,
        })
        .collect(),
    }
  }

  pub(super) fn registration(&self) -> (u32, usize) {
    (self.id, self.slots.len())
  }

  pub(super) fn recycle(
    &mut self,
    index: usize,
    retiring: bool,
  ) -> io::Result<()> {
    let slot = self
      .slots
      .get_mut(index)
      .ok_or_else(|| io::Error::other("unknown CAPTURE index"))?;
    let old = std::mem::replace(
      &mut slot.state,
      if retiring { SlotState::Idle } else { SlotState::Queued },
    );
    match old {
      SlotState::Held(Lease::Decoded(frame)) => drop(frame),
      SlotState::Held(Lease::Reserved(reservation)) => {
        drop(reservation)
      }
      _ => {}
    }
    Ok(())
  }

  pub(super) fn present(
    &mut self,
    frame: Frame,
  ) -> io::Result<Presentation> {
    let Some(index) = frame.pool_index() else {
      return Ok(Presentation::Convert(frame));
    };
    let Some(slot) = self.slots.get_mut(index) else {
      return Ok(Presentation::Convert(frame));
    };
    let Some(plane) = frame.plane()? else {
      return Ok(Presentation::Convert(frame));
    };
    if !matches!(slot.state, SlotState::Queued) {
      return Ok(Presentation::Convert(frame));
    }
    let same =
      if plane.fd().as_raw_fd() == slot.buffer.as_fd().as_raw_fd() {
        true
      } else {
        let src = system::metadata(plane.fd())?;
        let dst = system::metadata(slot.buffer.as_fd())?;
        src.st_dev == dst.st_dev && src.st_ino == dst.st_ino
      };
    if !same {
      return Ok(Presentation::Convert(frame));
    }
    slot.state = SlotState::Held(Lease::Decoded(frame));
    Ok(Presentation::Direct(index))
  }

  pub(super) fn prepare_write(
    &mut self,
    decoder: &mut Decoder,
    size: usize,
    stride: u32,
    rows: u32,
  ) -> io::Result<Option<Target<'_>>> {
    let reservation = if self.slots.iter().any(|slot| slot.imported) {
      let Some(lease) = decoder.reserve_capture(size)? else {
        return Ok(None);
      };
      Some(lease)
    } else {
      None
    };
    let index = if let Some(lease) = &reservation {
      lease.index()?
    } else {
      let Some(index) = self
        .slots
        .iter()
        .position(|s| matches!(s.state, SlotState::Queued))
      else {
        return Ok(None);
      };
      index
    };
    let Some(slot) = self.slots.get(index) else {
      return Err(io::Error::other(
        "MPP returned non-CAPTURE write reservation",
      ));
    };
    if !matches!(slot.state, SlotState::Queued) {
      return Ok(None);
    }
    if slot.buffer.size() < size {
      return Err(io::Error::other("CAPTURE write exceeds allocation"));
    }
    let image = Image::new(slot.buffer.clone(), stride, rows);
    Ok(Some(Target { pool: self, index, image, reservation }))
  }

  pub(super) fn reuse(
    &mut self,
    size: usize,
    reservations: Vec<Reservation>,
  ) -> io::Result<()> {
    if self.slots.iter().any(|s| s.buffer.size() < size) {
      return Err(io::Error::other(
        "capture pool is too small for new layout",
      ));
    }
    // Pin every shared surface before old display references can be
    // dropped. QBUF alone releases these pins; an empty LAST must
    // not create a writable hole.
    for reservation in reservations {
      let index = reservation.index()?;
      let slot = self.slots.get_mut(index).ok_or_else(|| {
        io::Error::other("invalid reused CAPTURE reservation")
      })?;
      slot.state = SlotState::Held(Lease::Reserved(reservation));
    }
    for slot in &mut self.slots {
      if matches!(slot.state, SlotState::Queued) {
        slot.state = SlotState::Idle;
      }
    }
    Ok(())
  }

  pub(super) fn forget_imports(&mut self) {
    for slot in &mut self.slots {
      slot.imported = false;
    }
  }

  pub(super) fn imports(&mut self) -> Vec<(usize, Buffer)> {
    self
      .slots
      .iter_mut()
      .enumerate()
      .filter_map(|(index, slot)| {
        if matches!(slot.state, SlotState::Queued) && !slot.imported {
          slot.imported = true;
          Some((index, slot.buffer.clone()))
        } else {
          None
        }
      })
      .collect()
  }
}
