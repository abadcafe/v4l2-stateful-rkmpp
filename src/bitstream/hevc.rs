//! module: bitstream::hevc; responsibility: HEVC picture output
//! eligibility. operations: Hevc implementing Syntax; entities: Hevc,
//! Pps; tests: hevc_tests, service output semantics.
use std::collections::BTreeMap;
use std::io;

use super::bits::Bits;
use super::codec::{NalFormat, Picture, Syntax, nals};
use super::hevc_rps::{self, Memory, Target};
use super::parameters::Parameters;
struct Pps {
  sequence: u32,
  output: bool,
  extra: usize,
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
  fn format(&self) -> NalFormat {
    NalFormat { kind: |b| (b >> 1) & 63, picture: |kind| kind <= 31 }
  }

  fn configuration(&self) -> Vec<u8> {
    self.sets.configuration()
  }

  fn boundary(&mut self) {
    self.seeking = true;
  }

  fn inspect(&mut self, bytes: &[u8]) -> io::Result<Option<Picture>> {
    let mut result = None;
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
          let output = b.get(1)? != 0;
          let extra = b.get(3)? as usize;
          if id > 63 || sequence > 15 {
            return Err(io::Error::other(
              "invalid HEVC parameter reference",
            ));
          }
          self.pictures.insert(id, Pps { sequence, output, extra });
          self.sets.insert(kind, id, nal);
        }
        0..=31 if result.is_none() => {
          if b.get(1)? == 0 {
            continue;
          }
          let irap = (16..=23).contains(&kind);
          let no_prior = irap && b.get(1)? != 0;
          let pps = self
            .pictures
            .get(&b.ue()?)
            .ok_or_else(|| io::Error::other("unknown HEVC PPS"))?;
          let seq = self
            .sequences
            .get(&pps.sequence)
            .ok_or_else(|| io::Error::other("unknown HEVC SPS"))?;
          if self.seeking && !irap {
            continue;
          }
          if matches!(kind, 8 | 9) && self.suppress_rasl {
            continue;
          }
          let reset = irap && (self.seeking || kind <= 20);
          if irap {
            self.suppress_rasl = reset;
          }
          if reset || self.highest.is_none() {
            self.highest = Some(seq.layers.len() - 1);
          }
          let highest = self.highest.unwrap();
          let limits = *seq.layers.get(highest).ok_or_else(|| {
            io::Error::other(
              "HEVC temporal operating point changed within CVSG",
            )
          })?;
          b.skip(pps.extra)?;
          if b.ue()? > 2 {
            return Err(io::Error::other("invalid HEVC slice type"));
          }
          let display = !pps.output || b.get(1)? != 0;
          if seq.separate {
            b.skip(2)?;
          }
          let lsb = if matches!(kind, 19 | 20) {
            0
          } else {
            b.get(seq.poc_bits)?
          };
          let poc = self.poc.derive(poc::Picture {
            lsb,
            bits: seq.poc_bits,
            kind,
            temporal_id: (nal[1] & 7).checked_sub(1).ok_or_else(
              || io::Error::other("invalid HEVC temporal id"),
            )?,
            random_access: reset,
          })?;
          let mut targets = Vec::new();
          if !matches!(kind, 19 | 20) {
            let deltas = if b.get(1)? == 0 {
              hevc_rps::parse(&mut b, &seq.sets, true)?
            } else {
              let index = if seq.sets.len() > 1 {
                b.get(
                  (usize::BITS - (seq.sets.len() - 1).leading_zeros())
                    as usize,
                )? as usize
              } else {
                0
              };
              seq.sets.get(index).cloned().ok_or_else(|| {
                io::Error::other("unknown HEVC reference set")
              })?
            };
            for delta in deltas {
              targets.push(Target::Short(
                poc.checked_add(delta).ok_or_else(|| {
                  io::Error::other("reference POC overflow")
                })?,
              ));
            }
            if let Some(long) = &seq.long_term {
              let from_sps =
                if long.is_empty() { 0 } else { b.ue()? as usize };
              let explicit = b.ue()? as usize;
              if from_sps > long.len() || from_sps + explicit > 16 {
                return Err(io::Error::other(
                  "invalid long-term reference count",
                ));
              }
              let mut cycle = 0u64;
              for i in 0..from_sps + explicit {
                let lsb = if i < from_sps {
                  let index = if long.len() > 1 {
                    b.get(
                      (usize::BITS - (long.len() - 1).leading_zeros())
                        as usize,
                    )? as usize
                  } else {
                    0
                  };
                  *long.get(index).ok_or_else(|| {
                    io::Error::other("invalid long-term SPS index")
                  })?
                } else {
                  let lsb = b.get(seq.poc_bits)?;
                  b.skip(1)?;
                  lsb
                };
                let present = b.get(1)? != 0;
                let delta =
                  if present { u64::from(b.ue()?) } else { 0 };
                cycle = if i == 0 || i == from_sps {
                  delta
                } else {
                  cycle.checked_add(delta).ok_or_else(|| {
                    io::Error::other("long-term cycle overflow")
                  })?
                };
                let modulus = 1u32 << seq.poc_bits;
                let full = if present {
                  let value = i128::from(lsb) + i128::from(poc)
                    - i128::from(cycle) * i128::from(modulus)
                    - i128::from(poc as u32 & (modulus - 1));
                  Some(i32::try_from(value).map_err(|_| {
                    io::Error::other("long-term POC overflow")
                  })?)
                } else {
                  None
                };
                targets.push(Target::Long {
                  lsb,
                  mask: modulus - 1,
                  full,
                });
              }
            }
          }
          let (id, references) =
            self.references.apply(poc, &targets, reset)?;
          let discard = reset && (no_prior || kind == 21);
          let mut geometry = seq.geometry;
          geometry.limits = limits;
          self.seeking = false;
          result = Some(Picture {
            geometry,
            order: poc,
            key: reset,
            reset_after: false,
            discard,
            display,
            part: crate::picture::Part::Frame,
            id,
            references,
            reference: true,
          });
        }
        _ => {}
      }
    }
    Ok(result)
  }
}

mod sequence;
#[cfg(test)]
mod sequence_tests;
use sequence::{Sequence, parse};

mod poc;
#[cfg(test)]
mod poc_tests;
