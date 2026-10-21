//! module: codec; responsibility: type the supported compressed
//! formats. operations: Codec conversion/fourcc; entities: Codec;
//! tests: protocol command admission and frontend acceptance.
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Codec {
  H264,
  Hevc,
  Vp8,
  Vp9,
  Av1,
}
impl Codec {
  pub(crate) fn fourcc(self) -> u32 {
    u32::from_le_bytes(*match self {
      Self::H264 => b"H264",
      Self::Hevc => b"HEVC",
      Self::Vp8 => b"VP80",
      Self::Vp9 => b"VP90",
      Self::Av1 => b"AV01",
    })
  }
}
impl TryFrom<u32> for Codec {
  type Error = io::Error;

  fn try_from(value: u32) -> io::Result<Self> {
    match &value.to_le_bytes() {
      b"H264" => Ok(Self::H264),
      b"HEVC" => Ok(Self::Hevc),
      b"VP80" => Ok(Self::Vp8),
      b"VP90" => Ok(Self::Vp9),
      b"AV01" => Ok(Self::Av1),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unsupported compressed format",
      )),
    }
  }
}
