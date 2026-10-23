//! module: codec_parser; responsibility: admit framed compressed input
//! using codec-specific syntax state. operations: new, validate, admit,
//! reset, configuration; entities: Validator; tests:
//! packet_syntax_tests and codec child tests. This parses
//! headers/partition bounds, never reconstructs pixels.
use std::io;
mod configuration;
#[cfg(test)]
mod configuration_tests;
pub use configuration::Configuration;
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
  /// Validate native syntax and the consumer's semantic admission as
  /// one transaction. A consumer rejection restores the previously
  /// committed configuration and discards prediction state before
  /// returning.
  pub fn admit<T>(
    &mut self,
    bytes: &[u8],
    budget: usize,
    inspect: impl FnOnce(&Self, unit::Admission) -> io::Result<T>,
  ) -> io::Result<T> {
    let configuration = self.configuration();
    let units = self.validate(bytes, budget)?;
    let result = inspect(self, units);
    if result.is_err() {
      if self.configuration() == configuration {
        self.reset()?;
      } else {
        let codec = match &self.0 {
          Codec::H264(_) => *b"H264",
          Codec::Hevc(_) => *b"HEVC",
          Codec::Vp8(_) => *b"VP80",
          Codec::Vp9(_) => *b"VP90",
          Codec::Av1(_) => *b"AV01",
        };
        *self = Self::new(u32::from_le_bytes(codec))?;
        if !configuration.is_empty() {
          self.validate(&configuration, 0)?;
        }
      }
    }
    result
  }

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

  /// Discard prediction state while retaining committed decoder
  /// configuration. Recovery and explicit seeking use the same
  /// codec-owned transaction.
  pub fn reset(&mut self) -> io::Result<()> {
    match &mut self.0 {
      Codec::H264(parser) => parser.reset(),
      Codec::Hevc(parser) => parser.reset(),
      Codec::Av1(parser) => parser.reset(),
      Codec::Vp8(parser) => {
        parser.reset();
        Ok(())
      }
      Codec::Vp9(parser) => parser.reset(),
    }
  }

  pub fn configuration(&self) -> Configuration {
    match &self.0 {
      Codec::Av1(parser) => parser.configuration(),
      Codec::H264(parser) => parser.configuration(),
      Codec::Hevc(parser) => parser.configuration(),
      _ => Configuration::default(),
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
  ) -> io::Result<unit::Admission> {
    // Each codec checks picture capacity before committing syntax
    // state.
    let units = match &mut self.0 {
      Codec::H264(parser) => {
        parser.validate(bytes, budget).map(|unit| vec![unit])
      }
      Codec::Hevc(parser) => {
        parser.validate(bytes, budget).map(|unit| vec![unit])
      }
      Codec::Vp8(parser) => {
        parser.validate(bytes, budget).map(|unit| vec![unit])
      }
      Codec::Vp9(parser) => parser.validate(bytes, budget),
      Codec::Av1(parser) => return parser.validate(bytes, budget),
    }?;
    Ok(unit::Admission { units, configures_decoder: false })
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
