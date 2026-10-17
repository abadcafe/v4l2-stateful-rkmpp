//! module: bitstream::parameters; responsibility: retain parameter sets
//! by identity. operations: insert, configuration; entities:
//! Parameters; tests: parameters_tests.
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Parameters(BTreeMap<(u8, u32), Vec<u8>>);
impl Parameters {
  pub(super) fn insert(&mut self, kind: u8, id: u32, nal: &[u8]) {
    self.0.insert((kind, id), nal.to_vec());
  }

  pub(super) fn configuration(&self) -> Vec<u8> {
    let mut result = Vec::new();
    for bytes in self.0.values() {
      result.extend([0, 0, 0, 1]);
      result.extend(bytes);
    }
    result
  }
}
