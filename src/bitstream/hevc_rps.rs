//! module: bitstream::hevc_rps; responsibility: HEVC reference-set
//! identity. operations: parse, Memory::apply, Target; entities:
//! Memory, Entry, Target; tests: hevc_rps_tests.
use std::io;

use super::bits::Bits;
#[derive(Clone, Copy)]
pub(super) enum Target {
  Short(i32),
  Long { lsb: u32, mask: u32, full: Option<i32> },
}
struct Entry {
  id: u64,
  poc: i32,
  long: bool,
}
#[derive(Default)]
pub(super) struct Memory {
  entries: Vec<Entry>,
  serial: u64,
}
impl Memory {
  pub(super) fn apply(
    &mut self,
    poc: i32,
    targets: &[Target],
    reset: bool,
    limits: crate::picture::Limits,
  ) -> io::Result<(u64, Vec<u64>)> {
    if reset {
      self.entries.clear();
    }
    let mut kept = Vec::new();
    for target in targets {
      if let Target::Long { lsb, mask, full } = *target {
        for entry in &mut self.entries {
          if full
            .map_or(entry.poc as u32 & mask == lsb, |p| p == entry.poc)
          {
            entry.long = true;
            kept.push(entry.id);
          }
        }
      }
    }
    for target in targets {
      if let Target::Short(poc) = *target {
        for entry in &self.entries {
          if !entry.long && entry.poc == poc {
            kept.push(entry.id);
          }
        }
      }
    }
    self.entries.retain(|e| kept.contains(&e.id));
    if self.entries.len() >= limits.capacity() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "HEVC reference set exceeds operating point capacity",
      ));
    }
    self.serial = self
      .serial
      .checked_add(1)
      .ok_or_else(|| io::Error::other("picture identity exhausted"))?;
    self.entries.push(Entry { id: self.serial, poc, long: false });
    Ok((self.serial, self.entries.iter().map(|e| e.id).collect()))
  }
}
/// Delta POCs include pictures not used by the current picture but
/// retained for future pictures (StFoll). UsedByCurrPic is not an
/// eligibility filter.
pub(super) fn parse(
  b: &mut Bits,
  prior: &[Vec<i32>],
  inline: bool,
) -> io::Result<Vec<i32>> {
  let mut deltas = Vec::new();
  if !prior.is_empty() && b.get(1)? != 0 {
    let distance = if inline { b.ue()? as usize + 1 } else { 1 };
    let source = prior
      .len()
      .checked_sub(distance)
      .and_then(|i| prior.get(i))
      .ok_or_else(|| {
        io::Error::other("invalid predicted reference set")
      })?;
    let sign = b.get(1)?;
    let magnitude = i32::try_from(b.ue_incremented()?)
      .map_err(|_| io::Error::other("reference delta overflow"))?;
    let delta = if sign == 0 { magnitude } else { -magnitude };
    for value in source.iter().copied().chain(std::iter::once(0)) {
      let used = b.get(1)? != 0;
      let retain = used || b.get(1)? != 0;
      let value = value
        .checked_add(delta)
        .ok_or_else(|| io::Error::other("reference POC overflow"))?;
      if retain && value != 0 {
        deltas.push(value);
      }
    }
    deltas.sort_by_key(|&n| {
      if n < 0 { (0, -i64::from(n)) } else { (1, i64::from(n)) }
    });
  } else {
    let negative = b.ue()?;
    let positive = b.ue()?;
    if u64::from(negative) + u64::from(positive) > 16 {
      return Err(io::Error::other("oversized HEVC reference set"));
    }
    let mut value = 0i32;
    for _ in 0..negative {
      value =
        value
          .checked_sub(i32::try_from(b.ue_incremented()?).map_err(
            |_| io::Error::other("reference delta overflow"),
          )?)
          .ok_or_else(|| io::Error::other("reference POC overflow"))?;
      b.skip(1)?;
      deltas.push(value);
    }
    value = 0;
    for _ in 0..positive {
      value =
        value
          .checked_add(i32::try_from(b.ue_incremented()?).map_err(
            |_| io::Error::other("reference delta overflow"),
          )?)
          .ok_or_else(|| io::Error::other("reference POC overflow"))?;
      b.skip(1)?;
      deltas.push(value);
    }
  }
  if deltas.len() > 16 {
    return Err(io::Error::other("oversized HEVC reference set"));
  }
  Ok(deltas)
}
