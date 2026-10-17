//! module: bitstream::codec; responsibility: Annex B picture semantics
//! contract. operations: Syntax, Picture, NalFormat; entities: those
//! contracts; tests: codec-specific public syntax tests and
//! bitstream_tests.
use std::io;

use super::geometry::Geometry;
#[derive(Clone, Copy)]
pub(super) struct NalFormat {
  pub kind: fn(u8) -> u8,
  pub picture: fn(u8) -> bool,
}
pub(super) struct Picture {
  pub geometry: Geometry,
  pub order: i32,
  pub key: bool,
  pub reset_after: bool,
  pub discard: bool,
  pub display: bool,
  pub part: crate::picture::Part,
  pub id: u64,
  pub references: Vec<u64>,
  pub reference: bool,
}
pub(super) trait Syntax {
  fn format(&self) -> NalFormat;
  fn configuration(&self) -> Vec<u8>;
  fn boundary(&mut self);
  fn inspect(&mut self, bytes: &[u8]) -> io::Result<Option<Picture>>;
}
pub(super) fn nals(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
  let starts: Vec<usize> = bytes
    .windows(3)
    .enumerate()
    .filter_map(|(i, w)| (w == [0, 0, 1]).then_some(i + 3))
    .collect();
  let mut nals = Vec::new();
  for (i, &start) in starts.iter().enumerate() {
    let end = starts.get(i + 1).map_or(bytes.len(), |p| p - 3);
    nals.push(&bytes[start..end]);
  }
  nals.into_iter()
}
