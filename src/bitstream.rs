//! module: bitstream; responsibility: interpret client-framed Annex B
//! submissions. operations: new, admit, seek; entities: Parser;
//! tests: bitstream_tests and codec semantic tests. No byte
//! accumulation or implicit frame boundaries are permitted across
//! OUTPUT buffers.
use std::io;

mod codec;
#[cfg(test)]
mod codec_tests;
mod h264;
#[cfg(test)]
mod h264_tests;
mod hevc;
#[cfg(test)]
mod hevc_irap_tests;
mod hevc_rps;
#[cfg(test)]
mod hevc_rps_tests;
#[cfg(test)]
mod hevc_temporal_tests;
#[cfg(test)]
mod hevc_tests;
mod recovery;
#[cfg(test)]
mod recovery_tests;
use codec::Syntax;
pub mod report;
pub use report::AccessUnit;
use report::Report;

use crate::submission::Prefix;
pub struct Parser {
  syntax: Box<dyn Syntax>,
  validator: codec_parser::Validator,
  codec: u32,
  restore_configuration: bool,
  metadata: Prefix,
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
      validator: codec_parser::Validator::new(codec)?,
      codec,
      restore_configuration: false,
      metadata: Prefix::default(),
    })
  }

  pub fn admit(&mut self, bytes: &[u8]) -> io::Result<Report> {
    let validated = match self.validator.validate(bytes, 1) {
      Ok(validated) => validated,
      Err(error) => {
        // Validator owns native syntax rollback. Its error already
        // leaves a restored configuration; only our presentation and
        // unsubmitted metadata still need recovery here.
        self.reset_syntax();
        return Err(error);
      }
    };
    match self.inspect(bytes, validated) {
      Ok(report) => Ok(report),
      Err(error) => {
        self.seek()?;
        Err(error)
      }
    }
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    validated: Vec<codec_parser::unit::Unit>,
  ) -> io::Result<Report> {
    let unit = validated
      .into_iter()
      .next()
      .ok_or_else(|| io::Error::other("missing Annex B admission"))?;
    let boundary = unit.boundary;
    let picture_bytes = &bytes[unit.range];
    let mut units = Vec::new();
    // Only syntax interpretation translates semantic failures into
    // damaged input. Native resource errors keep their classification
    // through the outer recovery transaction.
    let picture = self
      .syntax
      .inspect(
        &picture_bytes[..unit.prefix.min(picture_bytes.len())],
        unit.header.as_ref(),
      )
      .map_err(|error| {
        if matches!(
          error.kind(),
          io::ErrorKind::OutOfMemory | io::ErrorKind::InvalidData
        ) {
          error
        } else {
          io::Error::new(io::ErrorKind::InvalidData, error)
        }
      })?;
    if let Some(picture) = picture {
      let configuration = if self.restore_configuration {
        self.validator.configuration()
      } else {
        Vec::new()
      };
      // Restore only parameters absent from this AU's metadata. A
      // complete resume point already supplies the decoder
      // configuration: prepending it again would force an
      // otherwise borrowed picture into an owned whole-packet
      // assembly.
      let configuration = if configuration.is_empty() {
        configuration.as_slice()
      } else {
        let present: std::collections::BTreeSet<_> = codec::nals(
          &picture_bytes[..unit.prefix.min(picture_bytes.len())],
        )
        .collect();
        if codec::nals(&configuration)
          .all(|parameter| present.contains(parameter))
        {
          &[]
        } else {
          configuration.as_slice()
        }
      };
      let payload = self.metadata.compose(
        configuration,
        picture_bytes,
        0..picture_bytes.len(),
      )?;
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
      if unit.header.is_some() {
        self.metadata.clear();
        self.restore_configuration = true;
      } else {
        self.metadata.append(picture_bytes)?;
      }
    }
    if let Some(boundary) = boundary {
      self.syntax.boundary();
      self.metadata.clear();
      self.restore_configuration = true;
      return Ok(Report {
        units,
        end: boundary == codec_parser::unit::Boundary::Stream,
      });
    }
    Ok(Report { units, end: false })
  }

  pub fn seek(&mut self) -> io::Result<()> {
    let configuration = self.validator.configuration();
    let mut validator = codec_parser::Validator::new(self.codec)?;
    if !configuration.is_empty() {
      validator.validate(&configuration, 1)?;
    }
    self.validator = validator;
    self.reset_syntax();
    Ok(())
  }

  fn reset_syntax(&mut self) {
    self.syntax.seek();
    self.metadata.clear();
    self.restore_configuration = true;
  }
}
mod sei;
#[cfg(test)]
mod sei_tests;
