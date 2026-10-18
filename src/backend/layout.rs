//! module: backend::layout; responsibility: retain parsed geometry
//! and picture obligations across source changes. operations: push,
//! peek, discard_prior, complete, take, finish, drained; entities:
//! Layouts, Requirements, Picture; tests: layout_tests.
use std::collections::BTreeMap;
use std::io;

use crate::picture::Descriptor;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Requirements {
  pub width: u32,
  pub height: u32,
  pub coded_width: u32,
  pub coded_height: u32,
  pub minimum: u32,
  pub left: u32,
  pub top: u32,
}
impl Requirements {
  pub(super) fn from_frame(info: crate::mpp::FrameInfo) -> Self {
    Self {
      width: info.width,
      height: info.height,
      coded_width: info.width,
      coded_height: info.height,
      minimum: 18,
      left: 0,
      top: 0,
    }
  }
}
pub(super) struct Picture {
  pub requirements: Option<Requirements>,
  pub timestamp: u64,
  pub source: crate::timestamp::Source,
  pub plan: crate::picture::Plan,
}
#[derive(Default)]
pub(super) struct Layouts {
  pending: BTreeMap<u64, Picture>,
  pairs: BTreeMap<u64, u64>,
  unpaired: Option<(crate::picture::Id, u64)>,
  serial: u64,
}
impl Layouts {
  pub(super) fn push(&mut self, unit: &Descriptor) -> io::Result<u64> {
    let requirements = if let Some(g) = unit.geometry {
      if g.width == 0
        || g.height == 0
        || g.coded_width > 8192
        || g.coded_height > 8192
        || g.left.checked_add(g.width).is_none_or(|r| r > g.coded_width)
        || g
          .top
          .checked_add(g.height)
          .is_none_or(|r| r > g.coded_height)
      {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "invalid parsed geometry",
        ));
      }
      Some(Requirements {
        width: g.width,
        height: g.height,
        coded_width: g.coded_width,
        coded_height: g.coded_height,
        // MPP's decoder references and normative pending display
        // pictures may be different sets. Each is bounded by
        // the stream DPB capacity. Negotiate their union up
        // front instead of spilling outside CAPTURE.
        minimum: (2 * unit.plan.limits().capacity()).max(18) as u32,
        left: g.left,
        top: g.top,
      })
    } else {
      None
    };
    if self.pending.len() >= 4096 {
      return Err(io::Error::other(
        "too many access units without decoded frames",
      ));
    }
    self.serial = self
      .serial
      .checked_add(1)
      .ok_or_else(|| io::Error::other("frame serial exhausted"))?;
    self.pending.insert(
      self.serial,
      Picture {
        requirements,
        timestamp: unit.timestamp,
        source: unit.source,
        plan: unit.plan.clone(),
      },
    );
    match unit.plan.part() {
      crate::picture::Part::Continuation => {
        if let Some((identity, id)) = self.unpaired.take()
          && identity == unit.plan.id()
          && self.pending.contains_key(&id)
        {
          self.pairs.insert(id, self.serial);
          self.pairs.insert(self.serial, id);
        }
      }
      crate::picture::Part::FirstField => {
        self.unpaired = Some((unit.plan.id(), self.serial))
      }
      crate::picture::Part::Frame => self.unpaired = None,
    }
    Ok(self.serial)
  }

  pub(super) fn peek(
    &self,
    id: u64,
    width: u32,
    height: u32,
  ) -> io::Result<Requirements> {
    self
      .pending
      .get(&id)
      .filter(|p| {
        p.requirements
          .is_none_or(|e| e.width == width && e.height == height)
      })
      .map(|p| {
        p.requirements.unwrap_or(Requirements {
          width,
          height,
          coded_width: width,
          coded_height: height,
          minimum: 18,
          left: 0,
          top: 0,
        })
      })
      .ok_or_else(|| {
        io::Error::other("MPP layout has no parsed source")
      })
  }

  pub(super) fn discard_prior(&self, id: u64) -> io::Result<bool> {
    self
      .pending
      .get(&id)
      .map(|p| {
        p.plan.output().before == crate::picture::Boundary::Discard
      })
      .ok_or_else(|| {
        io::Error::other("MPP boundary has no parsed source")
      })
  }

  /// Consume metadata for a decoded picture, including a bufferless
  /// error. Empty successful notifications carry no picture and keep
  /// the ledger intact.
  pub(super) fn complete(
    &mut self,
    info: &mut crate::mpp::FrameInfo,
    image: bool,
  ) -> io::Result<Option<Picture>> {
    if info.info_change || (!image && !info.failed) {
      return Ok(None);
    }
    let picture = self.take(info.timestamp)?;
    info.timestamp = picture.timestamp;
    info.timestamp_source = picture.source as u32;
    Ok(Some(picture))
  }

  /// Reclaim completed hidden work. Keep pending presentations even
  /// when the public MPP output queue is empty, and keep a first
  /// field until its complementary field arrives.
  pub(super) fn finish(&mut self, id: u64) {
    if self.pending.get(&id).is_some_and(|p| {
      !p.plan.output().display
        && p.plan.part() != crate::picture::Part::FirstField
    }) {
      self.pending.remove(&id);
    }
  }

  /// Hardware idleness is not proof that every admitted display was
  /// delivered. Reject completion while any picture obligation remains.
  pub(super) fn drained(&self) -> io::Result<()> {
    if self.pending.is_empty() {
      Ok(())
    } else {
      Err(io::Error::other(format!(
        "decoder drain left {} unresolved picture obligations",
        self.pending.len()
      )))
    }
  }

  pub(super) fn take(&mut self, id: u64) -> io::Result<Picture> {
    let id = if let Some(other) = self.pairs.remove(&id) {
      self.pairs.remove(&other);
      if let Some(second) = self.pending.remove(&id.max(other))
        && let Some(first) = self.pending.get_mut(&id.min(other))
      {
        first.plan.complete_pair(&second.plan)?;
      }
      id.min(other)
    } else {
      id
    };
    self.pending.remove(&id).ok_or_else(|| {
      io::Error::other(format!(
        "MPP frame {id} has no live task metadata"
      ))
    })
  }
}
