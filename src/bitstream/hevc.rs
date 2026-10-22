//! module: bitstream::hevc; responsibility: HEVC picture output
//! eligibility. operations: Hevc implementing Syntax; entities: Hevc;
//! tests: hevc_tests, hevc_temporal_tests, hevc_irap_tests,
//! service output semantics.
use std::io;

use super::codec::{Picture, Syntax};
use super::hevc_rps::{
  Memory, Picture as ReferencePicture, References,
};
pub(super) struct Hevc {
  references: Memory,
  seeking: bool,
  suppress_rasl: bool,
  highest: Option<usize>,
  poc: poc::Counter,
}
impl Default for Hevc {
  fn default() -> Self {
    Self {
      references: Memory::default(),
      seeking: true,
      suppress_rasl: false,
      highest: None,
      poc: poc::Counter::default(),
    }
  }
}
impl Syntax for Hevc {
  fn seek(&mut self) {
    self.references.reset();
    self.seeking = true;
    self.suppress_rasl = false;
    self.highest = None;
    self.poc = poc::Counter::default();
  }

  fn boundary(&mut self) {
    self.seeking = true;
  }

  fn inspect(
    &mut self,
    _bytes: &[u8],
    header: Option<&codec_parser::header::Header>,
  ) -> io::Result<Option<Picture>> {
    let Some(codec_parser::header::Header::Hevc(header)) = header
    else {
      return Ok(None);
    };
    let kind = header.kind;
    let irap = (16..=23).contains(&kind);
    let seq = &header.sequence;
    if usize::from(header.temporal_id) >= seq.layers.len() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "HEVC TemporalId exceeds the active SPS layer bound",
      ));
    }
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
    let references = References::new(
      ReferencePicture {
        poc,
        kind,
        temporal_id: header.temporal_id,
        temporal_nested: seq.temporal_nested,
      },
      seq.poc_bits,
      &header.short,
      &header.long,
    )?;
    let (id, references) =
      self.references.apply(references, reset, limits)?;
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

mod poc;
#[cfg(test)]
mod poc_tests;
