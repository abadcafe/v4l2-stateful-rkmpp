//! module: packet_syntax; responsibility: admit framed compressed input
//! using codec-specific syntax state. operations: new, validate;
//! entities: Validator; tests: packet_syntax_tests and codec child
//! tests. This parses headers/partition bounds, never reconstructs
//! pixels.
use std::io;
mod av1;
#[cfg(test)]
mod av1_tests;
mod ffi;
mod vp8;
#[cfg(test)]
mod vp8_tests;
mod vp9;
#[cfg(test)]
mod vp9_tests;

pub(crate) struct Validator(Codec);
enum Codec {
  H264(h264::Validator),
  Hevc(hevc::Validator),
  Vp8(Box<vp8::Validator>),
  Vp9(vp9::Validator),
  Av1(Box<av1::Validator>),
}
impl Validator {
  pub(crate) fn new(codec: u32) -> io::Result<Self> {
    match &codec.to_le_bytes() {
      b"H264" => Ok(Self(Codec::H264(h264::Validator::new()?))),
      b"HEVC" => Ok(Self(Codec::Hevc(hevc::Validator::new()?))),
      b"VP80" => Ok(Self(Codec::Vp8(Box::new(vp8::Validator::new()?)))),
      b"VP90" => Ok(Self(Codec::Vp9(vp9::Validator::new()?))),
      b"AV01" => Ok(Self(Codec::Av1(Box::new(av1::Validator::new()?)))),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "not a framed codec",
      )),
    }
  }

  pub(crate) fn configuration(&self) -> Vec<u8> {
    match &self.0 {
      Codec::Av1(parser) => parser.configuration(),
      _ => Vec::new(),
    }
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
  ) -> io::Result<Vec<unit::Unit>> {
    match &mut self.0 {
      Codec::H264(parser) => {
        parser.validate(bytes).map(|unit| vec![unit])
      }
      Codec::Hevc(parser) => {
        parser.validate(bytes).map(|unit| vec![unit])
      }
      Codec::Vp8(parser) => {
        parser.validate(bytes).map(|unit| vec![unit])
      }
      Codec::Vp9(parser) => parser.validate(bytes),
      Codec::Av1(parser) => parser.validate(bytes),
    }
  }
}
mod h264;
#[cfg(test)]
mod h264_tests;
mod hevc;
#[cfg(test)]
mod hevc_tests;

pub(crate) mod unit;
#[cfg(test)]
mod unit_tests;

pub(crate) mod header;

#[cfg(test)]
mod header_tests;
