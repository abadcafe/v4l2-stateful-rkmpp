//! module: bitstream::h264::order; responsibility: detect H264
//! picture-order reset boundaries. operations: observe;
//! entities: Order, Marking; tests: h264_order_tests,
//! bitstream_tests.
use std::io;

use codec_parser::header::{
  H264, Marking as HeaderMarking, Operation as HeaderOperation,
};

use super::reference::{
  Marking as ReferenceMarking, Memory, Picture as ReferencePicture,
};
#[derive(Debug, Default)]
pub(super) struct Marking {
  pub geometry: codec_parser::syntax::geometry::Geometry,
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
  memory: Memory,
  poc: super::poc::Counter,
}
impl Order {
  pub(super) fn reset(&mut self) {
    self.memory.reset();
    self.poc = super::poc::Counter::default();
  }

  pub(super) fn observe(
    &mut self,
    header: &H264,
  ) -> io::Result<Marking> {
    let s = &header.sequence;
    let reference_marking = match &header.marking {
      HeaderMarking::Disposable => ReferenceMarking::Disposable,
      HeaderMarking::Idr { long, .. } => {
        ReferenceMarking::Idr { long: *long }
      }
      HeaderMarking::Sliding => ReferenceMarking::Sliding,
      HeaderMarking::Adaptive(operations) => {
        ReferenceMarking::Adaptive(operations)
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
  parameters: &codec_parser::syntax::h264::Poc,
  mut poc: super::poc::Picture,
  mut marking: Marking,
  reference: ReferencePicture<'_>,
) -> io::Result<Marking> {
  poc.reset = marking.reset;
  marking.poc = counter.derive(parameters, poc)?;
  let state = memory.apply(reference)?;
  marking.reference_id = state.id;
  marking.references = state.references;
  marking.continuation = state.continuation;
  Ok(marking)
}
