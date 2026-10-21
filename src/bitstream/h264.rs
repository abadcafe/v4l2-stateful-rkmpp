//! module: bitstream::h264; responsibility: H264 access-unit
//! eligibility. operations: H264 implementing Syntax; entities: H264;
//! tests: h264_tests, bitstream_tests, service output semantics.
use std::io;

use order::Order;

use super::codec::{Picture, Syntax, nals};
use super::recovery::{Point, Recovery};
pub(super) struct H264 {
  order: Order,
  recovery: Recovery,
  pending: Option<Point>,
  seeking: bool,
}
impl Default for H264 {
  fn default() -> Self {
    Self {
      order: Order::default(),
      recovery: Recovery::default(),
      pending: None,
      seeking: true,
    }
  }
}
impl Syntax for H264 {
  fn seek(&mut self) {
    self.order.reset();
    self.recovery = Recovery::default();
    self.pending = None;
    self.seeking = true;
  }

  fn boundary(&mut self) {
    self.seeking = true;
    self.pending = None;
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    header: Option<&codec_parser::header::Header>,
  ) -> io::Result<Option<Picture>> {
    let mut point = self.pending.take();
    for nal in nals(bytes) {
      let Some((&header, data)) = nal.split_first() else { continue };
      let kind = header & 31;
      if kind == 6 {
        point = Point::parse(data)?.or(point);
      }
    }
    let Some(codec_parser::header::Header::H264(header)) = header
    else {
      self.pending = point;
      return Ok(None);
    };
    let m = self.order.observe(header)?;
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

mod order;
#[cfg(test)]
mod order_tests;
mod reference;
#[cfg(test)]
mod reference_tests;

mod poc;
#[cfg(test)]
mod poc_tests;
