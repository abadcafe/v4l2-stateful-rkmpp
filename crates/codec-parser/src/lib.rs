//! module: codec_parser; responsibility: admit framed compressed input
//! using codec-specific syntax state. operations: new, validate,
//! configuration; entities: Validator; tests: packet_syntax_tests and
//! codec child tests. This parses headers/partition bounds, never
//! reconstructs pixels.
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

pub struct Validator(Codec);
enum Codec {
  H264(h264::Validator),
  Hevc(hevc::Validator),
  Vp8(vp8::Validator),
  Vp9(vp9::Validator),
  Av1(Box<av1::Validator>),
}
impl Validator {
  pub fn new(codec: u32) -> io::Result<Self> {
    match &codec.to_le_bytes() {
      b"H264" => Ok(Self(Codec::H264(h264::Validator::new()?))),
      b"HEVC" => Ok(Self(Codec::Hevc(hevc::Validator::new()?))),
      b"VP80" => Ok(Self(Codec::Vp8(vp8::Validator::new()?))),
      b"VP90" => Ok(Self(Codec::Vp9(vp9::Validator::new()?))),
      b"AV01" => Ok(Self(Codec::Av1(Box::new(av1::Validator::new()?)))),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "not a framed codec",
      )),
    }
  }

  pub fn configuration(&self) -> Vec<u8> {
    match &self.0 {
      Codec::Av1(parser) => parser.configuration(),
      Codec::H264(parser) => parser.configuration(),
      Codec::Hevc(parser) => parser.configuration(),
      _ => Vec::new(),
    }
  }

  /// Admit one framed submission. Invalid syntax discards prediction
  /// state while retaining the last committed decoder configuration;
  /// callers can admit a new resume point without rebuilding this
  /// owner. Budget rejection has the same prediction recovery policy.
  /// Resource failures are fatal and may prevent native restoration.
  pub fn validate(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<Vec<unit::Unit>> {
    // Annex B always returns a unit, including metadata. Budget
    // rejection cannot commit its parameter changes. Framed codecs
    // own their prediction recovery even on an exhausted budget; AV1
    // may still admit metadata without a frame.
    if budget == 0 && matches!(&self.0, Codec::H264(_) | Codec::Hevc(_))
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "coded frame budget exhausted",
      ));
    }
    match &mut self.0 {
      Codec::H264(parser) => {
        parser.validate(bytes).map(|unit| vec![unit])
      }
      Codec::Hevc(parser) => {
        parser.validate(bytes).map(|unit| vec![unit])
      }
      Codec::Vp8(parser) => {
        parser.validate(bytes, budget).map(|unit| vec![unit])
      }
      Codec::Vp9(parser) => parser.validate(bytes, budget),
      Codec::Av1(parser) => parser.validate(bytes, budget),
    }
  }
}
mod h264;
#[cfg(test)]
mod h264_tests;
mod hevc;
#[cfg(test)]
mod hevc_tests;

pub mod unit;
#[cfg(test)]
mod unit_tests;

pub mod header;

#[cfg(test)]
mod header_tests;

mod limits;
pub mod syntax;
pub use limits::Limits;
#[cfg(test)]
mod limits_tests;

#[cfg(test)]
mod lib_tests;
