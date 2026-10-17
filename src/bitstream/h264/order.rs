//! module: bitstream::h264::order; responsibility: detect H264
//! picture-order reset boundaries. operations: observe; entities:
//! Order, Sequence, Picture; tests: h264_order_tests, bitstream_tests.
use std::collections::BTreeMap;
use std::io;

use super::reference::{
  Marking as ReferenceMarking, Memory, Operation,
  Picture as ReferencePicture,
};
use super::sequence::{PictureOrder, Sequence};
use crate::bitstream::bits::Bits;
use crate::bitstream::parameters::Parameters;
#[derive(Clone, Copy)]
struct Picture {
  sequence: u32,
  bottom_poc: bool,
  refs: [u32; 2],
  weighted: bool,
  bipred: u32,
  redundant: bool,
}
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
  picture: BTreeMap<u32, Picture>,
  memory: Memory,
  poc: super::poc::Counter,
  sets: Parameters,
}
fn picture(b: &mut Bits) -> io::Result<(u32, Picture)> {
  let id = b.ue()?;
  let sequence = b.ue()?;
  b.skip(1)?;
  let bottom_poc = b.get(1)? != 0;
  let groups = b.ue()?;
  if id > 255 || sequence > 31 || groups > 7 {
    return Err(io::Error::other("invalid picture parameters"));
  }
  if groups != 0 {
    match b.ue()? {
      0 => {
        for _ in 0..=groups {
          b.ue()?;
        }
      }
      2 => {
        for _ in 0..groups {
          b.ue()?;
          b.ue()?;
        }
      }
      3..=5 => {
        b.skip(1)?;
        b.ue()?;
      }
      6 => {
        let size = b.ue()?;
        if size > 1 << 20 {
          return Err(io::Error::other("oversized slice group map"));
        }
        let bits = (32 - groups.leading_zeros()) as usize;
        b.skip((size as usize + 1) * bits)?;
      }
      1 => {}
      _ => return Err(io::Error::other("invalid slice groups")),
    }
  }
  let refs = [b.ue_incremented()?, b.ue_incremented()?];
  let weighted = b.get(1)? != 0;
  let bipred = b.get(2)?;
  b.se()?;
  b.se()?;
  b.se()?;
  b.skip(2)?;
  let redundant = b.get(1)? != 0;
  if refs.iter().any(|&n| n > 32) {
    return Err(io::Error::other("invalid reference count"));
  }
  Ok((
    id,
    Picture { sequence, bottom_poc, refs, weighted, bipred, redundant },
  ))
}
fn list(b: &mut Bits) -> io::Result<()> {
  if b.get(1)? == 0 {
    return Ok(());
  }
  for _ in 0..64 {
    match b.ue()? {
      0..=2 => {
        b.ue()?;
      }
      3 => return Ok(()),
      _ => break,
    }
  }
  Err(io::Error::other("invalid reference list modification"))
}
fn weights(b: &mut Bits, chroma: bool, refs: &[u32]) -> io::Result<()> {
  b.ue()?;
  if chroma {
    b.ue()?;
  }
  for &count in refs {
    if count > 32 {
      return Err(io::Error::other("invalid weighted reference count"));
    }
    for _ in 0..count {
      if b.get(1)? != 0 {
        b.se()?;
        b.se()?;
      }
      if chroma && b.get(1)? != 0 {
        for _ in 0..2 {
          b.se()?;
          b.se()?;
        }
      }
    }
  }
  Ok(())
}
impl Order {
  pub(super) fn configuration(&self) -> Vec<u8> {
    self.sets.configuration()
  }

  pub(super) fn observe(
    &mut self,
    nal: &[u8],
  ) -> io::Result<Option<Marking>> {
    let Some((&header, data)) = nal.split_first() else {
      return Ok(None);
    };
    let kind = header & 31;
    let mut b = Bits::new(data);
    if kind == 7 {
      let s = super::sequence::parse(data)?;
      self.sets.insert(kind, s.geometry.id, nal);
      self.sequence.insert(s.geometry.id, s);
      return Ok(None);
    }
    if kind == 8 {
      let (id, p) = picture(&mut b)?;
      self.sets.insert(kind, id, nal);
      self.picture.insert(id, p);
      return Ok(None);
    }
    if !matches!(kind, 1 | 2 | 5) {
      return Ok(None);
    }
    b.ue()?;
    let slice = b.ue()? % 5;
    let id = b.ue()?;
    let p = self
      .picture
      .get(&id)
      .ok_or_else(|| io::Error::other("unknown PPS"))?;
    let s = self
      .sequence
      .get(&p.sequence)
      .ok_or_else(|| io::Error::other("unknown SPS"))?;
    if s.separate {
      b.skip(2)?;
    }
    let frame = b.get(s.frame_bits)?;
    let field = !s.frame_only && b.get(1)? != 0;
    let bottom = if field { Some(b.get(1)? != 0) } else { None };
    if kind == 5 {
      b.ue()?;
    }
    let mut marking = Marking {
      geometry: s.geometry,
      frame,
      modulus: 1 << s.frame_bits,
      field: bottom,
      reference: header & 96 != 0,
      idr: kind == 5,
      ..Default::default()
    };
    let mut poc = super::poc::Picture {
      frame,
      frame_bits: s.frame_bits,
      lsb: 0,
      delta: [0, 0],
      bottom_delta: 0,
      field: match bottom {
        None => super::poc::Field::Frame,
        Some(false) => super::poc::Field::Top,
        Some(true) => super::poc::Field::Bottom,
      },
      reference: marking.reference,
      idr: marking.idr,
      reset: false,
    };
    match s.order {
      PictureOrder::Lsb(bits) => {
        poc.lsb = b.get(bits)?;
        if p.bottom_poc && !field {
          poc.bottom_delta = b.se()?;
        }
      }
      PictureOrder::Delta { always_zero: false } => {
        poc.delta[0] = b.se()?;
        if p.bottom_poc && !field {
          poc.delta[1] = b.se()?;
        }
      }
      _ => {}
    }
    if p.redundant {
      b.ue()?;
    }
    if slice == 1 {
      b.skip(1)?;
    }
    let mut refs = p.refs;
    if matches!(slice, 0 | 1 | 3) && b.get(1)? != 0 {
      refs[0] = b.ue_incremented()?;
      if slice == 1 {
        refs[1] = b.ue_incremented()?;
      }
    }
    if !matches!(slice, 2 | 4) {
      list(&mut b)?;
      if slice == 1 {
        list(&mut b)?;
      }
    }
    if (p.weighted && matches!(slice, 0 | 3))
      || (p.bipred == 1 && slice == 1)
    {
      weights(
        &mut b,
        s.chroma != 0,
        &refs[..if slice == 1 { 2 } else { 1 }],
      )?;
    }
    let mut reference = ReferencePicture {
      frame,
      modulus: marking.modulus,
      field: bottom,
      maximum: s.maximum,
      gaps_allowed: s.gaps_allowed,
      marking: ReferenceMarking::Disposable,
    };
    if !marking.reference {
      return Ok(Some(record(
        &mut self.memory,
        &mut self.poc,
        &s.poc,
        poc,
        marking,
        reference,
      )?));
    }
    if marking.idr {
      marking.discard = b.get(1)? != 0;
      reference.marking =
        ReferenceMarking::Idr { long: b.get(1)? != 0 };
      return Ok(Some(record(
        &mut self.memory,
        &mut self.poc,
        &s.poc,
        poc,
        marking,
        reference,
      )?));
    }
    if b.get(1)? == 0 {
      reference.marking = ReferenceMarking::Sliding;
      return Ok(Some(record(
        &mut self.memory,
        &mut self.poc,
        &s.poc,
        poc,
        marking,
        reference,
      )?));
    }
    let mut operations = Vec::new();
    for _ in 0..64 {
      let operation = match b.ue()? {
        0 => {
          reference.marking = ReferenceMarking::Adaptive(operations);
          return Ok(Some(record(
            &mut self.memory,
            &mut self.poc,
            &s.poc,
            poc,
            marking,
            reference,
          )?));
        }
        1 => Operation::ForgetShort(b.ue_incremented()?),
        2 => Operation::ForgetLong(b.ue()?),
        3 => Operation::LongFromShort(b.ue_incremented()?, b.ue()?),
        4 => Operation::LongLimit(b.ue()?),
        5 => {
          marking.reset = true;
          Operation::Reset
        }
        6 => Operation::CurrentLong(b.ue()?),
        _ => break,
      };
      operations.push(operation);
    }
    Err(io::Error::other("invalid reference marking"))
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
