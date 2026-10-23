//! module: reference::field; responsibility: validate
//! complementary-field reference assignments. operations: complete,
//! promote; entities: Current, Reference, Promotion; tests:
//! field_tests. Views carry only marking facts, independently of the
//! DPB container and its mutation strategy.
use std::io;

use codec_parser::header::Operation;

use super::super::number::Number;

#[derive(Clone, Copy)]
pub(super) struct Current {
  pub id: u64,
  pub field: bool,
  pub required_long: Option<u32>,
  pub assigned_long: Option<u32>,
}
pub(super) struct Reference {
  pub id: u64,
  pub long: Option<u32>,
}
#[derive(Clone, Copy)]
pub(super) struct Promotion {
  pub number: Number,
  pub target: i64,
  pub index: u32,
  pub short_complement: bool,
  pub current_pair: bool,
}
pub(super) fn complete(
  current: Current,
  references: impl IntoIterator<Item = Reference>,
) -> io::Result<()> {
  if (current.required_long.is_some()
    && current.required_long != current.assigned_long)
    || (current.field
      && references
        .into_iter()
        .any(|r| r.id == current.id && r.long != current.assigned_long))
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "H264 complementary fields have inconsistent reference classes",
    ));
  }
  Ok(())
}
pub(super) fn promote(
  p: Promotion,
  operations: &[Operation],
) -> io::Result<()> {
  let current = 2 * i64::from(p.number.value()) + 1;
  if (p.short_complement || p.current_pair)
    && !operations.iter().any(|operation| match *operation {
      Operation::LongFromShort { difference, index } => {
        index == p.index
          && current - i64::from(difference) - 1 == (p.target ^ 1)
      }
      Operation::CurrentLong(index) => {
        p.current_pair && index == p.index
      }
      _ => false,
    })
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "H264 promotion lacks its complementary-field assignment",
    ));
  }
  Ok(())
}
