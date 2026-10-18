//! module: bitstream::hevc; responsibility: HEVC picture output
//! eligibility. operations: Hevc implementing Syntax; entities: Hevc,
//! Pps; tests: hevc_tests, service output semantics.
use std::collections::BTreeMap;
use std::io;

use super::bits::Bits;
use super::codec::{Picture, Syntax, nals};
use super::hevc_rps::{Memory, Target};
use super::parameters::Parameters;
struct Pps {
  sequence: u32,
}
pub(super) struct Hevc {
  sets: Parameters,
  sequences: BTreeMap<u32, Sequence>,
  pictures: BTreeMap<u32, Pps>,
  references: Memory,
  seeking: bool,
  suppress_rasl: bool,
  highest: Option<usize>,
  poc: poc::Counter,
}
impl Default for Hevc {
  fn default() -> Self {
    Self {
      sets: Parameters::default(),
      sequences: BTreeMap::new(),
      pictures: BTreeMap::new(),
      references: Memory::default(),
      seeking: true,
      suppress_rasl: false,
      highest: None,
      poc: poc::Counter::default(),
    }
  }
}
impl Syntax for Hevc {
  fn configuration(&self) -> Vec<u8> {
    self.sets.configuration()
  }

  fn boundary(&mut self) {
    self.seeking = true;
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    header: Option<&crate::packet_syntax::header::Header>,
  ) -> io::Result<Option<Picture>> {
    for nal in nals(bytes) {
      if nal.len() < 2 {
        continue;
      }
      let kind = (nal[0] >> 1) & 63;
      let mut b = Bits::new(&nal[2..]);
      match kind {
        32 => {
          let id = b.get(4)?;
          self.sets.insert(kind, id, nal);
        }
        33 => {
          let seq = parse(&nal[2..])?;
          self.sets.insert(kind, seq.geometry.id, nal);
          self.sequences.insert(seq.geometry.id, seq);
        }
        34 => {
          let id = b.ue()?;
          let sequence = b.ue()?;
          b.skip(1)?;
          b.skip(4)?;
          if id > 63 || sequence > 15 {
            return Err(io::Error::other(
              "invalid HEVC parameter reference",
            ));
          }
          self.pictures.insert(id, Pps { sequence });
          self.sets.insert(kind, id, nal);
        }
        _ => {}
      }
    }
    let Some(crate::packet_syntax::header::Header::Hevc(header)) =
      header
    else {
      return Ok(None);
    };
    let kind = header.kind;
    let irap = (16..=23).contains(&kind);
    let pps = self
      .pictures
      .get(&header.pps)
      .ok_or_else(|| io::Error::other("unknown HEVC PPS"))?;
    let seq = self
      .sequences
      .get(&pps.sequence)
      .ok_or_else(|| io::Error::other("unknown HEVC SPS"))?;
    if (self.seeking && !irap)
      || (matches!(kind, 8 | 9) && self.suppress_rasl)
    {
      return Ok(None);
    }
    let reset = irap && (self.seeking || kind <= 20);
    if irap {
      self.suppress_rasl = reset;
    }
    if reset || self.highest.is_none() {
      self.highest = Some(seq.layers.len() - 1);
    }
    let limits =
      *seq.layers.get(self.highest.unwrap()).ok_or_else(|| {
        io::Error::other(
          "HEVC temporal operating point changed within CVSG",
        )
      })?;
    let poc = self.poc.derive(poc::Picture {
      lsb: header.lsb,
      bits: seq.poc_bits,
      kind,
      temporal_id: header.temporal_id,
      random_access: reset,
    })?;
    let mut targets =
      Vec::with_capacity(header.short.len() + header.long.len());
    for delta in &header.short {
      targets.push(Target::Short(
        poc
          .checked_add(*delta)
          .ok_or_else(|| io::Error::other("reference POC overflow"))?,
      ));
    }
    let modulus = 1u32 << seq.poc_bits;
    for long in &header.long {
      let full = long
        .cycle
        .map(|cycle| {
          i32::try_from(
            i128::from(long.lsb) + i128::from(poc)
              - i128::from(cycle) * i128::from(modulus)
              - i128::from(poc as u32 & (modulus - 1)),
          )
          .map_err(|_| io::Error::other("long-term POC overflow"))
        })
        .transpose()?;
      targets.push(Target::Long {
        lsb: long.lsb,
        mask: modulus - 1,
        full,
      });
    }
    let (id, references) =
      self.references.apply(poc, &targets, reset)?;
    let mut geometry = seq.geometry;
    geometry.limits = limits;
    self.seeking = false;
    Ok(Some(Picture {
      geometry,
      order: poc,
      key: reset,
      reset_after: false,
      discard: reset && (header.no_prior || kind == 21),
      display: header.display,
      part: crate::picture::Part::Frame,
      id,
      references,
      reference: true,
    }))
  }
}

mod sequence;
#[cfg(test)]
mod sequence_tests;
use sequence::{Sequence, parse};
mod poc;
#[cfg(test)]
mod poc_tests;
