//! module: configuration; responsibility: retain immutable decoder
//! configuration snapshots. operations: commit, configuration,
//! as_slice; entities: Parameters, Configuration; tests:
//! configuration_tests. A rejected packet never changes saved
//! configuration.
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::io;
use std::ops::Deref;
use std::rc::Rc;

/// An immutable decoder configuration, shared across admission and
/// recovery.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Configuration(Option<Rc<Vec<u8>>>);
impl Configuration {
  pub(crate) fn new(bytes: Vec<u8>) -> Self {
    Self((!bytes.is_empty()).then(|| Rc::new(bytes)))
  }

  pub fn as_slice(&self) -> &[u8] {
    self.0.as_deref().map_or(&[], Vec::as_slice)
  }
}
impl Deref for Configuration {
  type Target = [u8];

  fn deref(&self) -> &[u8] {
    self.as_slice()
  }
}
#[derive(Default)]
pub(crate) struct Parameters {
  sets: BTreeMap<(u8, u32), Vec<u8>>,
  size: usize,
  snapshot: OnceCell<Configuration>,
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
        .filter(|&size| size <= MAX_CONFIGURATION_SIZE)
        .ok_or_else(|| {
          io::Error::new(
            io::ErrorKind::InvalidData,
            "parameter configuration exceeds compressed task limit",
          )
        })?;
    }
    if self.size != size {
      self.snapshot.take();
    }
    for (key, bytes) in updates {
      let stored = self.sets.entry(key).or_default();
      if stored.as_slice() == bytes {
        continue;
      }
      self.snapshot.take();
      if stored.len() > bytes.len() {
        *stored = bytes.to_vec();
      } else {
        stored.clear();
        stored.reserve_exact(bytes.len());
        stored.extend_from_slice(bytes);
      }
    }
    self.size = size;
    Ok(())
  }

  pub(crate) fn configuration(&self) -> Configuration {
    self
      .snapshot
      .get_or_init(|| {
        let mut result = Vec::with_capacity(self.size);
        for bytes in self.sets.values() {
          result.extend([0, 0, 0, 1]);
          result.extend(bytes);
        }
        Configuration::new(result)
      })
      .clone()
  }
}

/// Bound retained parameter data independently of hardware decoding.
pub(crate) const MAX_CONFIGURATION_SIZE: usize = 16 * 1024 * 1024;
