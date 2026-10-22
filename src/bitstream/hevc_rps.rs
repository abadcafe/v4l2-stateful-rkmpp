//! module: bitstream::hevc_rps; responsibility: admit HEVC
//! reference-set identities. operations: References new, Memory
//! reset/apply; entities: Picture, References, Memory, Entry, Irap;
//! tests: hevc_rps_tests, hevc_tests, hevc_temporal_tests,
//! hevc_irap_tests.
use std::io;

use codec_parser::header::{Long, Short};

#[derive(Clone, Copy)]
pub(super) struct Picture {
  pub poc: i32,
  pub kind: u8,
  pub temporal_id: u8,
  pub temporal_nested: bool,
}
pub(super) struct References<'a> {
  picture: Picture,
  mask: u32,
  short: &'a [Short],
  long: &'a [Long],
}
impl<'a> References<'a> {
  pub(super) fn new(
    picture: Picture,
    bits: usize,
    short: &'a [Short],
    long: &'a [Long],
  ) -> io::Result<Self> {
    let poc = picture.poc;
    if picture.kind > 31
      || picture.temporal_id > 6
      || (matches!(picture.kind, 2..=5) && picture.temporal_id == 0)
      || ((16..=23).contains(&picture.kind) && picture.temporal_id != 0)
      || (picture.temporal_nested
        && picture.temporal_id != 0
        && !matches!(picture.kind, 2 | 3 | 6..=9))
      || !(4..=16).contains(&bits)
      || short.len() > 16
      || long.len() > 16
      || short.len() + long.len() > 16
    {
      return Err(invalid(
        "HEVC reference syntax outside storage domain",
      ));
    }
    let references =
      Self { picture, mask: (1 << bits) - 1, short, long };
    for (index, entry) in short.iter().enumerate() {
      if entry.delta == 0
        || poc.checked_add(entry.delta).is_none()
        || short[..index].iter().any(|other| other.delta == entry.delta)
      {
        return Err(invalid(
          "invalid or duplicate HEVC short-term reference",
        ));
      }
    }
    for (index, entry) in long.iter().enumerate() {
      if entry.lsb > references.mask
        || references
          .long_poc(entry)
          .is_some_and(|poc| i32::try_from(poc).is_err())
      {
        return Err(invalid(
          "HEVC long-term reference POC outside domain",
        ));
      }
      if short.iter().any(|other| {
        references.long_matches(
          entry,
          i128::from(poc) + i128::from(other.delta),
        )
      }) || long[..index].iter().any(|other| {
        if entry.cycle.is_none() || other.cycle.is_none() {
          entry.lsb == other.lsb
        } else {
          references.long_poc(entry) == references.long_poc(other)
        }
      }) {
        return Err(invalid(
          "HEVC reference appears in multiple RPS entries",
        ));
      }
    }
    Ok(references)
  }

  fn short_matches(&self, target: &Short, poc: i32) -> bool {
    i64::from(poc)
      == i64::from(self.picture.poc) + i64::from(target.delta)
  }

  fn long_poc(&self, target: &Long) -> Option<i128> {
    target.cycle.map(|cycle| {
      i128::from(target.lsb) + i128::from(self.picture.poc)
        - i128::from(cycle) * i128::from(self.mask + 1)
        - i128::from(self.picture.poc as u32 & self.mask)
    })
  }

  fn long_matches(&self, target: &Long, poc: i128) -> bool {
    self
      .long_poc(target)
      .map_or(poc as u32 & self.mask == target.lsb, |full| full == poc)
  }
}
struct Entry {
  id: u64,
  poc: i32,
  long: bool,
  kind: u8,
  temporal_id: u8,
}
struct Irap {
  id: u64,
  poc: i32,
  maximum: i32,
}
#[derive(Default)]
pub(super) struct Memory {
  entries: Vec<Entry>,
  serial: u64,
  stsa: [u64; 7],
  lower: [u64; 7],
  irap: Option<Irap>,
}
impl Memory {
  pub(super) fn reset(&mut self) {
    self.entries.clear();
    self.stsa.fill(0);
    self.lower.fill(0);
    self.irap = None;
  }

  pub(super) fn apply(
    &mut self,
    references: References<'_>,
    reset: bool,
    limits: codec_parser::Limits,
  ) -> io::Result<(u64, Vec<u64>)> {
    let entries = if reset { &[] } else { self.entries.as_slice() };
    // H.265 8.3.2: select/validate the long-term RPS before promotion,
    // then resolve the short-term RPS. Missing following pictures are
    // permitted, while every picture used by the current picture is
    // required.
    for target in references.long {
      let mut count = 0;
      for entry in entries.iter().filter(|entry| {
        references.long_matches(target, i128::from(entry.poc))
      }) {
        if i64::from(references.picture.poc) - i64::from(entry.poc)
          >= 1 << 24
        {
          return Err(invalid(
            "HEVC long-term reference exceeds POC distance limit",
          ));
        }
        self.eligible(references.picture, entry, target.used)?;
        count += 1;
      }
      if count > 1 {
        return Err(invalid("ambiguous HEVC long-term reference POC"));
      }
      if target.used && count == 0 {
        return Err(invalid(
          "required HEVC long-term reference is absent",
        ));
      }
    }
    for target in references.short {
      let found = entries.iter().find(|entry| {
        !entry.long && references.short_matches(target, entry.poc)
      });
      if let Some(entry) = found {
        self.eligible(references.picture, entry, target.used)?;
      } else if target.used {
        return Err(invalid(
          "required HEVC short-term reference is absent",
        ));
      }
    }
    let serial = self
      .serial
      .checked_add(1)
      .ok_or_else(|| invalid("picture identity exhausted"))?;
    if reset {
      self.reset();
    }
    self.entries.retain_mut(|entry| {
      let long = references.long.iter().any(|target| {
        references.long_matches(target, i128::from(entry.poc))
      });
      if long {
        entry.long = true;
      }
      long
        || (!entry.long
          && references
            .short
            .iter()
            .any(|target| references.short_matches(target, entry.poc)))
    });
    if self.entries.len() >= limits.capacity() {
      return Err(invalid(
        "HEVC reference set exceeds operating point capacity",
      ));
    }
    self.serial = serial;
    self.entries.push(Entry {
      id: serial,
      poc: references.picture.poc,
      long: false,
      kind: references.picture.kind,
      temporal_id: references.picture.temporal_id,
    });
    if (16..=23).contains(&references.picture.kind) {
      let poc = references.picture.poc;
      self.irap = Some(Irap {
        id: serial,
        poc,
        maximum: self
          .irap
          .as_ref()
          .map_or(poc, |previous| previous.maximum.max(poc)),
      });
    }
    if matches!(references.picture.kind, 4 | 5) {
      self.stsa[usize::from(references.picture.temporal_id)] = serial;
    }
    // Keep decoding-order history for each candidate reference layer;
    // validation consults it only when temporal nesting is enabled.
    self.lower[usize::from(references.picture.temporal_id) + 1..]
      .fill(serial);
    Ok((serial, self.entries.iter().map(|entry| entry.id).collect()))
  }

  fn eligible(
    &self,
    current: Picture,
    entry: &Entry,
    used: bool,
  ) -> io::Result<()> {
    // H.265 8.3.2: CRA concerns any earlier IRAP; trailing pictures
    // concern the associated IRAP. RADL's decoding boundary applies
    // only to references used by the current picture.
    if let Some(irap) = &self.irap {
      let forbidden = match current.kind {
        21 => entry.id < irap.id || entry.poc < irap.maximum,
        0..=5 => entry.id < irap.id || entry.poc < irap.poc,
        6 | 7 if used => entry.id < irap.id,
        _ => false,
      };
      if forbidden {
        return Err(invalid("HEVC reference crosses an IRAP boundary"));
      }
    }
    if matches!(current.kind, 6 | 7)
      && used
      && matches!(entry.kind, 8 | 9)
    {
      return Err(invalid("HEVC RADL cannot use a RASL reference"));
    }
    // H.265 8.3.2: TSA restricts every retained reference; the other
    // temporal restrictions concern references used by this picture.
    if matches!(current.kind, 2 | 3)
      && entry.temporal_id >= current.temporal_id
    {
      return Err(invalid(
        "HEVC TSA reference is not from a lower temporal layer",
      ));
    }
    if used {
      if entry.temporal_id > current.temporal_id {
        return Err(invalid(
          "HEVC current reference comes from a higher temporal layer",
        ));
      }
      // H.265 8.3.2: nested prediction cannot cross a picture whose
      // TemporalId is lower than that of the candidate reference.
      if current.temporal_nested
        && entry.id < self.lower[usize::from(entry.temporal_id)]
      {
        return Err(invalid(
          "HEVC nested reference crosses a lower temporal layer",
        ));
      }
      if entry.temporal_id == current.temporal_id
        && (matches!(entry.kind, 0 | 2 | 4 | 6 | 8)
          || matches!(current.kind, 4 | 5)
          || entry.id < self.stsa[usize::from(current.temporal_id)])
      {
        return Err(invalid(
          "HEVC current reference violates a sub-layer boundary",
        ));
      }
    }
    Ok(())
  }
}
fn invalid(message: &'static str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message)
}
