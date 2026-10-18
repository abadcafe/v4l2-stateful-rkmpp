//! module: bitstream::parameters; responsibility: retain parameter sets
//! by identity within the compressed-task budget. operations: insert,
//! configuration; entities:
//! Parameters; tests: parameters_tests.
use std::collections::BTreeMap;
use std::io;
#[derive(Default)]
pub(super) struct Parameters {
  sets: BTreeMap<(u8, u32), Vec<u8>>,
  size: usize,
}
impl Parameters {
  pub(super) fn insert(
    &mut self,
    kind: u8,
    id: u32,
    nal: &[u8],
  ) -> io::Result<()> {
    let old =
      self.sets.get(&(kind, id)).map_or(0, |bytes| 4 + bytes.len());
    let size = (self.size - old)
      .checked_add(4)
      .and_then(|size| size.checked_add(nal.len()))
      .filter(|&size| size <= crate::submission::MAX_PACKET_SIZE)
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidData,
          "parameter configuration exceeds compressed task limit",
        )
      })?;
    let stored = self.sets.entry((kind, id)).or_default();
    if stored.len() > nal.len() {
      // Shrinking must release capacity: otherwise cycling large sets
      // through different identities evades the aggregate byte budget.
      *stored = nal.to_vec();
    } else if stored.as_slice() != nal {
      stored.clear();
      stored.reserve_exact(nal.len());
      stored.extend_from_slice(nal);
    }
    self.size = size;
    Ok(())
  }

  pub(super) fn configuration(&self) -> Vec<u8> {
    let mut result = Vec::with_capacity(self.size);
    for bytes in self.sets.values() {
      result.extend([0, 0, 0, 1]);
      result.extend(bytes);
    }
    result
  }
}
