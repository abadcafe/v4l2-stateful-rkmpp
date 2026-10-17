//! module: bitstream; responsibility: interpret client-framed Annex B
//! submissions. operations: new, admit, seek; entities: Parser;
//! tests: bitstream_tests and codec semantic tests. No byte
//! accumulation or implicit frame boundaries are permitted across
//! OUTPUT buffers.
use std::io;

mod bits;
#[cfg(test)]
mod bits_tests;
mod codec;
#[cfg(test)]
mod codec_tests;
mod geometry;
mod h264;
#[cfg(test)]
mod h264_tests;
mod hevc;
mod hevc_rps;
#[cfg(test)]
mod hevc_rps_tests;
#[cfg(test)]
mod hevc_tests;
mod parameters;
#[cfg(test)]
mod parameters_tests;
mod recovery;
#[cfg(test)]
mod recovery_tests;
mod restriction;
#[cfg(test)]
mod restriction_tests;
use codec::Syntax;
pub mod report;
pub use report::AccessUnit;
use report::Report;

use crate::submission::Payload;
pub struct Parser {
  syntax: Box<dyn Syntax>,
  validator: crate::packet_syntax::Validator,
  codec: u32,
  restore_configuration: bool,
  metadata: Vec<u8>,
}
impl Parser {
  pub fn new(codec: u32) -> io::Result<Self> {
    let syntax: Box<dyn Syntax> = match &codec.to_le_bytes() {
      b"H264" => Box::new(h264::H264::default()),
      b"HEVC" => Box::new(hevc::Hevc::default()),
      _ => {
        return Err(io::Error::new(
          io::ErrorKind::InvalidInput,
          "not an Annex B codec",
        ));
      }
    };
    Ok(Self {
      syntax,
      validator: crate::packet_syntax::Validator::new(codec)?,
      codec,
      restore_configuration: false,
      metadata: Vec::new(),
    })
  }

  pub fn admit(&mut self, bytes: &[u8]) -> io::Result<Report> {
    match self.inspect(bytes) {
      Ok(report) => Ok(report),
      Err(error) => {
        self.seek()?;
        Err(io::Error::new(
          io::ErrorKind::InvalidData,
          error.to_string(),
        ))
      }
    }
  }

  fn inspect(&mut self, bytes: &[u8]) -> io::Result<Report> {
    let validated = self.validator.validate(bytes)?;
    let unit = validated
      .into_iter()
      .next()
      .ok_or_else(|| io::Error::other("missing Annex B admission"))?;
    let format = self.syntax.format();
    let boundary = unit.boundary;
    let picture_bytes = &bytes[unit.range];
    let mut units = Vec::new();
    let semantic = if self.metadata.is_empty() {
      std::borrow::Cow::Borrowed(picture_bytes)
    } else {
      let mut data = self.metadata.clone();
      data.extend_from_slice(picture_bytes);
      std::borrow::Cow::Owned(data)
    };
    if let Some(picture) = self.syntax.inspect(&semantic)? {
      let payload =
        if self.restore_configuration || !self.metadata.is_empty() {
          let mut data = if self.restore_configuration {
            self.syntax.configuration()
          } else {
            Vec::new()
          };
          data.extend_from_slice(&semantic);
          Payload::Owned(data)
        } else {
          Payload::Input(0..picture_bytes.len())
        };
      self.metadata.clear();
      self.restore_configuration = false;
      let geometry = picture.geometry;
      units.push(AccessUnit {
        payload,
        geometry: crate::picture::Geometry {
          width: geometry.width,
          height: geometry.height,
          coded_width: geometry.coded_width,
          coded_height: geometry.coded_height,
          left: geometry.left,
          top: geometry.top,
        },
        plan: crate::picture::Plan::new(
          crate::picture::Id::new(picture.id),
          picture.part,
          picture
            .references
            .into_iter()
            .map(crate::picture::Id::new)
            .collect(),
          geometry.limits,
          picture.reference,
          crate::picture::Output {
            order: picture.order,
            display: picture.display,
            before: if picture.discard {
              crate::picture::Boundary::Discard
            } else if picture.key {
              crate::picture::Boundary::Drain
            } else {
              crate::picture::Boundary::Continue
            },
            drain_after: picture.reset_after,
          },
        ),
      });
    } else {
      if codec::nals(picture_bytes).any(|nal| {
        nal
          .first()
          .is_some_and(|byte| (format.picture)((format.kind)(*byte)))
      }) {
        self.metadata.clear();
      } else {
        if self
          .metadata
          .len()
          .checked_add(picture_bytes.len())
          .is_none_or(|n| n > crate::submission::MAX_PACKET_SIZE)
        {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "metadata prefix exceeds input limit",
          ));
        }
        self.metadata.extend_from_slice(picture_bytes);
      }
      self.restore_configuration = true;
    }
    if let Some(boundary) = boundary {
      self.syntax.boundary();
      self.metadata.clear();
      self.restore_configuration = true;
      return Ok(Report {
        units,
        end: boundary == crate::packet_syntax::unit::Boundary::Stream,
      });
    }
    Ok(Report { units, end: false })
  }

  pub fn seek(&mut self) -> io::Result<()> {
    let configuration = self.syntax.configuration();
    let mut next = Self::new(self.codec)?;
    if !configuration.is_empty() {
      next.validator.validate(&configuration)?;
      next.syntax.inspect(&configuration)?;
    }
    next.restore_configuration = true;
    *self = next;
    Ok(())
  }
}
mod sei;
#[cfg(test)]
mod sei_tests;
