//! module: backend::capture; responsibility: enforce CAPTURE
//! write/display ownership. operations: registration/import/recycle,
//! present, prepare_write, reuse; entities: CapturePool, Target,
//! Presentation, Slot, SlotState, Lease; tests: capture_tests. Target
//! borrows the pool exclusively until commit or drop.
use std::io;
use std::num::NonZeroU32;
use std::os::fd::{AsFd, AsRawFd};

use crate::dma::Buffer;
use crate::image::Plane;
use crate::mpp::{Decoder, Frame, Reservation};
enum Lease {
  Decoded(Frame),
  Reserved(Reservation),
  Private,
}
enum SlotState {
  Idle,
  Queued(NonZeroU32),
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
#[derive(Clone, Copy)]
pub(super) enum CopyReason {
  Unregistered,
  Held,
  Foreign,
}
pub(super) enum Presentation {
  Direct(usize, NonZeroU32),
  Convert(Frame, CopyReason),
}
pub(super) struct Target<'a> {
  pool: &'a mut CapturePool,
  index: usize,
  stride: u32,
  rows: u32,
  reservation: Option<Reservation>,
  token: NonZeroU32,
}
impl Target<'_> {
  pub(super) fn plane(&self) -> Plane<'_> {
    Plane::from_buffer(
      &self.pool.slots[self.index].buffer,
      self.stride,
      self.rows,
    )
  }

  pub(super) fn commit(self) -> (usize, NonZeroU32) {
    self.pool.slots[self.index].state = SlotState::Held(
      self.reservation.map_or(Lease::Private, Lease::Reserved),
    );
    (self.index, self.token)
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
    token: NonZeroU32,
    retiring: bool,
  ) -> io::Result<()> {
    let slot = self
      .slots
      .get_mut(index)
      .ok_or_else(|| io::Error::other("unknown CAPTURE index"))?;
    let old = std::mem::replace(
      &mut slot.state,
      if retiring { SlotState::Idle } else { SlotState::Queued(token) },
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
      return Ok(Presentation::Convert(
        frame,
        CopyReason::Unregistered,
      ));
    };
    let Some(slot) = self.slots.get_mut(index) else {
      return Ok(Presentation::Convert(
        frame,
        CopyReason::Unregistered,
      ));
    };
    let Some(plane) = frame.plane()? else {
      return Ok(Presentation::Convert(
        frame,
        CopyReason::Unregistered,
      ));
    };
    let same =
      if plane.fd().as_raw_fd() == slot.buffer.as_fd().as_raw_fd() {
        true
      } else {
        let src = plane.metadata()?;
        let dst = slot.buffer.metadata();
        src.st_dev == dst.st_dev && src.st_ino == dst.st_ino
      };
    if !same {
      return Ok(Presentation::Convert(frame, CopyReason::Foreign));
    }
    let SlotState::Queued(token) = slot.state else {
      return Ok(Presentation::Convert(frame, CopyReason::Held));
    };
    slot.state = SlotState::Held(Lease::Decoded(frame));
    Ok(Presentation::Direct(index, token))
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
        .position(|s| matches!(s.state, SlotState::Queued(_)))
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
    let SlotState::Queued(token) = slot.state else {
      return Ok(None);
    };
    if slot.buffer.size() < size {
      return Err(io::Error::other("CAPTURE write exceeds allocation"));
    }
    Ok(Some(Target {
      pool: self,
      index,
      stride,
      rows,
      reservation,
      token,
    }))
  }

  pub(super) fn fits(&self, count: usize, size: usize) -> bool {
    self.slots.len() >= count
      && self.slots.iter().all(|s| s.buffer.size() >= size)
  }

  pub(super) fn reuse(
    &mut self,
    size: usize,
    reservations: Option<Vec<Reservation>>,
  ) -> io::Result<()> {
    if self.slots.iter().any(|s| s.buffer.size() < size) {
      return Err(io::Error::other(
        "capture pool is too small for new layout",
      ));
    }
    // Pin every shared surface before old display references can be
    // dropped. QBUF alone releases these pins; an empty LAST must
    // not create a writable hole.
    let retain_imports = reservations.is_some();
    for reservation in reservations.into_iter().flatten() {
      let index = reservation.index()?;
      let slot = self.slots.get_mut(index).ok_or_else(|| {
        io::Error::other("invalid reused CAPTURE reservation")
      })?;
      slot.state = SlotState::Held(Lease::Reserved(reservation));
    }
    for slot in &mut self.slots {
      if !retain_imports {
        slot.imported = false;
      }
      if matches!(slot.state, SlotState::Queued(_)) {
        slot.state = SlotState::Idle;
      }
    }
    Ok(())
  }

  pub(super) fn imports(&mut self) -> Vec<(usize, Buffer)> {
    self
      .slots
      .iter_mut()
      .enumerate()
      .filter_map(|(index, slot)| {
        if matches!(slot.state, SlotState::Queued(_)) && !slot.imported
        {
          slot.imported = true;
          Some((index, slot.buffer.clone()))
        } else {
          None
        }
      })
      .collect()
  }
}
