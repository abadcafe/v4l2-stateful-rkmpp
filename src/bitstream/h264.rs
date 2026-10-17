//! module: bitstream::h264; responsibility: H264 access-unit
//! eligibility. operations: H264 implementing Syntax; entities: H264;
//! tests: h264_tests, bitstream_tests, service output semantics.
use std::io;

use order::Order;

use super::codec::{NalFormat, Picture, Syntax, nals};
use super::recovery::{Point, Recovery};
pub(super) struct H264 {
  order: Order,
  recovery: Recovery,
  seeking: bool,
}
impl Default for H264 {
  fn default() -> Self {
    Self {
      order: Order::default(),
      recovery: Recovery::default(),
      seeking: true,
    }
  }
}
impl Syntax for H264 {
  fn format(&self) -> NalFormat {
    NalFormat {
      kind: |b| b & 31,
      picture: |kind| (1..=5).contains(&kind),
    }
  }

  fn configuration(&self) -> Vec<u8> {
    self.order.configuration()
  }

  fn boundary(&mut self) {
    self.seeking = true;
  }

  fn inspect(&mut self, bytes: &[u8]) -> io::Result<Option<Picture>> {
    let mut marking = None;
    let mut point = None;
    for nal in nals(bytes) {
      let Some((&header, data)) = nal.split_first() else { continue };
      let kind = header & 31;
      if kind == 6 {
        point = Point::parse(data)?.or(point);
      }
      if (marking.is_none() || matches!(kind, 7 | 8))
        && let Some(m) = self.order.observe(nal)?
      {
        marking = Some(m);
      }
    }
    let Some(m) = marking else {
      return Ok(None);
    };
    let mut discard = m.discard;
    if m.idr {
      self.recovery = Recovery::default();
    } else if let Some(point) = point
      && (self.seeking || point.broken)
    {
      self.recovery.begin(point, m.frame, m.modulus);
      discard |= point.broken;
    }
    let display = self.recovery.picture(m.frame, m.poc, m.reference);
    if m.reset {
      self.recovery = Recovery::default();
    }
    if self.seeking && !m.idr && point.is_none() {
      return Ok(None);
    }
    self.seeking = false;
    Ok(Some(Picture {
      geometry: m.geometry,
      order: m.poc,
      key: m.idr,
      reset_after: m.reset,
      discard,
      display,
      part: if m.continuation {
        crate::picture::Part::Continuation
      } else if m.field.is_some() {
        crate::picture::Part::FirstField
      } else {
        crate::picture::Part::Frame
      },
      id: m.reference_id,
      references: m.references,
      reference: m.reference,
    }))
  }
}

mod sequence;
#[cfg(test)]
mod sequence_tests;

mod order;
#[cfg(test)]
mod order_tests;
mod reference;
#[cfg(test)]
mod reference_tests;

mod poc;
#[cfg(test)]
mod poc_tests;
