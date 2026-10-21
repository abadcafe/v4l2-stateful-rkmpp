//! module: bitstream::hevc_rps; responsibility: HEVC reference-set
//! identity. operations: parse, Memory::apply, Target; entities:
//! Memory, Entry, Target; tests: hevc_rps_tests.
use std::io;

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
  pub(super) fn reset(&mut self) {
    self.entries.clear();
  }

  pub(super) fn apply(
    &mut self,
    poc: i32,
    targets: &[Target],
    reset: bool,
    limits: codec_parser::Limits,
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
