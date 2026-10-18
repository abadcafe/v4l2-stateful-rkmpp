//! module: bitstream::codec; responsibility: Annex B picture semantics
//! contract. operations: Syntax, Picture, nals; entities: those
//! contracts; tests: codec-specific public syntax tests and
//! bitstream_tests.
use std::io;

use super::geometry::Geometry;
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
  fn configuration(&self) -> Vec<u8>;
  fn boundary(&mut self);
  fn inspect(
    &mut self,
    bytes: &[u8],
    header: Option<&crate::packet_syntax::header::Header>,
  ) -> io::Result<Option<Picture>>;
}
pub(super) fn nals(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
  let mut remaining = bytes;
  let mut start =
    remaining.windows(3).position(|w| w == [0, 0, 1]).map(|i| i + 3);
  std::iter::from_fn(move || {
    let begin = start?;
    remaining = &remaining[begin..];
    let end = remaining.windows(3).position(|w| w == [0, 0, 1]);
    start = end.map(|i| i + 3);
    let payload = &remaining[..end.unwrap_or(remaining.len())];
    // Annex B's zero_byte and trailing_zero_8bits belong to framing,
    // not to the parameter NAL. Retaining them makes every serialized
    // configuration acquire another zero on the next seek. This view
    // is used only for metadata semantics; the original coded packet
    // (including any CABAC zero words) still goes to MPP unchanged.
    let size = payload
      .iter()
      .rposition(|&byte| byte != 0)
      .map_or(0, |position| position + 1);
    Some(&payload[..size])
  })
}
