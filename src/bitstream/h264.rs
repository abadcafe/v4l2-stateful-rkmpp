//! module: bitstream::h264; responsibility: H264 access-unit
//! eligibility. operations: H264 implementing Syntax; entities: H264;
//! tests: h264_tests, bitstream_tests, service output semantics.
use std::io;

use order::Order;

use super::codec::{Picture, Syntax, nals};
mod number;
#[cfg(test)]
mod number_tests;
mod recovery;
#[cfg(test)]
mod recovery_tests;
use recovery::{Point, Recovery};
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
    // A terminated coded sequence has no reference or frame-number
    // history for the next recovery point. Keep only the monotonically
    // increasing picture identities, as for an explicit seek.
    self.seek();
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
    // Leading pictures after seek have no decoded reference history.
    // Decide eligibility before executing their marking commands or
    // advancing POC/identity state; missing old references are
    // expected.
    if self.seeking
      && !matches!(
        header.marking,
        codec_parser::header::Marking::Idr { .. }
      )
      && point.is_none()
    {
      return Ok(None);
    }
    let m = self.order.observe(header)?;
    let mut discard = m.discard;
    if m.idr {
      self.recovery = Recovery::default();
    } else if let Some(point) = point
      && (self.seeking || point.broken)
    {
      self.recovery.begin(point, m.number);
      discard |= point.broken;
    }
    let display = self.recovery.picture(m.number, m.poc, m.reference);
    if m.reset {
      self.recovery = Recovery::default();
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
mod reference_identity_tests;
#[cfg(test)]
mod reference_tests;

mod poc;
#[cfg(test)]
mod poc_tests;
