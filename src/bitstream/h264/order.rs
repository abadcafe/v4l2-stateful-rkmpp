//! module: bitstream::h264::order; responsibility: detect H264
//! picture-order reset boundaries. operations: configure, observe,
//! configuration; entities: Order, Marking; tests: h264_order_tests,
//! bitstream_tests.
use std::collections::BTreeMap;
use std::io;

use super::reference::{
  Marking as ReferenceMarking, Memory, Operation,
  Picture as ReferencePicture,
};
use super::sequence::Sequence;
use crate::bitstream::bits::Bits;
use crate::bitstream::parameters::Parameters;
use crate::packet_syntax::header::{
  H264, Marking as HeaderMarking, Operation as HeaderOperation,
};
#[derive(Debug, Default)]
pub(super) struct Marking {
  pub geometry: crate::bitstream::geometry::Geometry,
  pub reset: bool,
  pub poc: i32,
  pub discard: bool,
  pub frame: u32,
  pub modulus: u32,
  pub field: Option<bool>,
  pub reference: bool,
  pub idr: bool,
  pub reference_id: u64,
  pub continuation: bool,
  pub references: Vec<u64>,
}
#[derive(Default)]
pub(super) struct Order {
  sequence: BTreeMap<u32, Sequence>,
  picture: BTreeMap<u32, u32>,
  memory: Memory,
  poc: super::poc::Counter,
  sets: Parameters,
}
impl Order {
  pub(super) fn configuration(&self) -> Vec<u8> {
    self.sets.configuration()
  }

  pub(super) fn configure(&mut self, nal: &[u8]) -> io::Result<()> {
    let Some((&header, data)) = nal.split_first() else {
      return Ok(());
    };
    let kind = header & 31;
    if kind == 7 {
      let s = super::sequence::parse(data)?;
      self.sets.insert(kind, s.geometry.id, nal)?;
      self.sequence.insert(s.geometry.id, s);
    } else if kind == 8 {
      let mut bits = Bits::new(data);
      let id = bits.ue()?;
      let sequence = bits.ue()?;
      if id > 255 || sequence > 31 {
        return Err(io::Error::other(
          "invalid H264 parameter identity",
        ));
      }
      self.sets.insert(kind, id, nal)?;
      self.picture.insert(id, sequence);
    }
    Ok(())
  }

  pub(super) fn observe(
    &mut self,
    header: &H264,
  ) -> io::Result<Marking> {
    let sequence = self
      .picture
      .get(&header.pps)
      .ok_or_else(|| io::Error::other("unknown PPS"))?;
    let s = self
      .sequence
      .get(sequence)
      .ok_or_else(|| io::Error::other("unknown SPS"))?;
    let reference_marking = match &header.marking {
      HeaderMarking::Disposable => ReferenceMarking::Disposable,
      HeaderMarking::Idr { long, .. } => {
        ReferenceMarking::Idr { long: *long }
      }
      HeaderMarking::Sliding => ReferenceMarking::Sliding,
      HeaderMarking::Adaptive(operations) => {
        ReferenceMarking::Adaptive(
          operations
            .iter()
            .map(|op| {
              Ok(match *op {
                HeaderOperation::ForgetShort(n) => {
                  Operation::ForgetShort(n.checked_add(1).ok_or_else(
                    || {
                      io::Error::other("reference difference overflow")
                    },
                  )?)
                }
                HeaderOperation::ForgetLong(n) => {
                  Operation::ForgetLong(n)
                }
                HeaderOperation::LongFromShort {
                  difference,
                  index,
                } => Operation::LongFromShort(
                  difference.checked_add(1).ok_or_else(|| {
                    io::Error::other("reference difference overflow")
                  })?,
                  index,
                ),
                HeaderOperation::LongLimit(n) => {
                  Operation::LongLimit(n)
                }
                HeaderOperation::Reset => Operation::Reset,
                HeaderOperation::CurrentLong(n) => {
                  Operation::CurrentLong(n)
                }
              })
            })
            .collect::<io::Result<Vec<_>>>()?,
        )
      }
    };
    let reset = matches!(&header.marking, HeaderMarking::Adaptive(operations) if operations.contains(&HeaderOperation::Reset));
    let marking = Marking {
      geometry: s.geometry,
      reset,
      frame: header.frame,
      modulus: 1 << s.frame_bits,
      field: header.field,
      reference: !matches!(header.marking, HeaderMarking::Disposable),
      idr: matches!(header.marking, HeaderMarking::Idr { .. }),
      discard: matches!(
        header.marking,
        HeaderMarking::Idr { discard: true, .. }
      ),
      ..Default::default()
    };
    let poc = super::poc::Picture {
      frame: header.frame,
      frame_bits: s.frame_bits,
      lsb: header.lsb,
      delta: header.delta,
      bottom_delta: header.bottom_delta,
      field: match header.field {
        None => super::poc::Field::Frame,
        Some(false) => super::poc::Field::Top,
        Some(true) => super::poc::Field::Bottom,
      },
      reference: marking.reference,
      idr: marking.idr,
      reset,
    };
    let reference = ReferencePicture {
      frame: header.frame,
      modulus: marking.modulus,
      field: header.field,
      maximum: s.maximum,
      gaps_allowed: s.gaps_allowed,
      marking: reference_marking,
    };
    record(
      &mut self.memory,
      &mut self.poc,
      &s.poc,
      poc,
      marking,
      reference,
    )
  }
}

fn record(
  memory: &mut Memory,
  counter: &mut super::poc::Counter,
  parameters: &super::poc::Parameters,
  mut poc: super::poc::Picture,
  mut marking: Marking,
  reference: ReferencePicture,
) -> io::Result<Marking> {
  poc.reset = marking.reset;
  marking.poc = counter.derive(parameters, poc)?;
  let state = memory.apply(reference)?;
  marking.reference_id = state.id;
  marking.references = state.references;
  marking.continuation = state.continuation;
  Ok(marking)
}
