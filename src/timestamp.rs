//! module: timestamp; responsibility: type V4L2 timestamp origins.
//! operations: Source conversion; entities: Source; tests:
//! timestamp_tests.
use std::io;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum Source {
  #[default]
  End = 0,
  Start = 0x10000,
}
impl TryFrom<u32> for Source {
  type Error = io::Error;

  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(Self::End),
      0x10000 => Ok(Self::Start),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid timestamp source",
      )),
    }
  }
}
