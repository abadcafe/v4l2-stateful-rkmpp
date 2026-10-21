//! module: syntax::parameters; responsibility: commit bounded parameter
//! snapshots. operations: commit, configuration; entities: Parameters;
//! tests: parameters_tests. A rejected packet never changes saved
//! configuration.
use std::collections::BTreeMap;
use std::io;
#[derive(Default)]
pub(crate) struct Parameters {
  sets: BTreeMap<(u8, u32), Vec<u8>>,
  size: usize,
}
impl Parameters {
  pub(crate) fn commit(
    &mut self,
    changes: &[(u8, u32, &[u8])],
  ) -> io::Result<()> {
    let updates: BTreeMap<(u8, u32), &[u8]> = changes
      .iter()
      .map(|&(kind, id, bytes)| ((kind, id), bytes))
      .collect();
    let removed: usize = updates
      .keys()
      .map(|key| self.sets.get(key).map_or(0, |bytes| bytes.len() + 4))
      .sum();
    let mut size = self.size - removed;
    for bytes in updates.values() {
      size = size
        .checked_add(4)
        .and_then(|size| size.checked_add(bytes.len()))
        .filter(|&size| {
          size <= crate::syntax::parameters::MAX_CONFIGURATION_SIZE
        })
        .ok_or_else(|| {
          io::Error::new(
            io::ErrorKind::InvalidData,
            "parameter configuration exceeds compressed task limit",
          )
        })?;
    }
    for (key, bytes) in updates {
      let stored = self.sets.entry(key).or_default();
      if stored.len() > bytes.len() {
        *stored = bytes.to_vec();
      } else if stored.as_slice() != bytes {
        stored.clear();
        stored.reserve_exact(bytes.len());
        stored.extend_from_slice(bytes);
      }
    }
    self.size = size;
    Ok(())
  }

  pub(crate) fn configuration(&self) -> Vec<u8> {
    let mut result = Vec::with_capacity(self.size);
    for bytes in self.sets.values() {
      result.extend([0, 0, 0, 1]);
      result.extend(bytes);
    }
    result
  }
}

/// Bound retained parameter data independently of hardware decoding.
pub(crate) const MAX_CONFIGURATION_SIZE: usize = 16 * 1024 * 1024;
