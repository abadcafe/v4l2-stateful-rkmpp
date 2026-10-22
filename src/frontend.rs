//! module: frontend; responsibility: turn framed codec input into
//! uniform decode tasks. operations: new, admit, seek; entities:
//! Frontend, Codec, Admission, Task, Payload; tests: frontend_tests.
use std::io;

use codec_parser::Limits;
use rkmpp::Payload;

use crate::picture::{Descriptor, Id, Output, Part, Plan};
use crate::timestamp::Source;
pub(crate) struct Task {
  pub payload: Payload,
  pub descriptor: Descriptor,
}
pub(crate) struct Admission {
  pub tasks: Vec<Task>,
  pub end: bool,
}
enum Codec {
  Annexb(crate::bitstream::Parser),
  Framed(codec_parser::Validator),
}
pub(crate) struct Frontend {
  codec: Codec,
  fourcc: u32,
  serial: u64,
  metadata: crate::submission::Prefix,
  configuration: Vec<u8>,
}
impl Frontend {
  pub(crate) fn new(fourcc: u32) -> io::Result<Self> {
    let codec = match &fourcc.to_le_bytes() {
      b"H264" | b"HEVC" => {
        Codec::Annexb(crate::bitstream::Parser::new(fourcc)?)
      }
      _ => Codec::Framed(codec_parser::Validator::new(fourcc)?),
    };
    Ok(Self {
      codec,
      fourcc,
      serial: 0,
      metadata: crate::submission::Prefix::default(),
      configuration: Vec::new(),
    })
  }

  pub(crate) fn admit(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
    budget: usize,
  ) -> io::Result<Admission> {
    let result = self.inspect(bytes, timestamp, source, budget.min(32));
    // Annex B owns its metadata/recovery transaction in Parser. Framed
    // codecs retain metadata here, so reset that whole transaction on
    // any rejected submission, not only on assembled-task overflow.
    if result.is_err() && matches!(self.codec, Codec::Framed(_)) {
      self.seek()?;
    }
    result
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
    budget: usize,
  ) -> io::Result<Admission> {
    match &mut self.codec {
      Codec::Annexb(parser) => {
        let report = parser.admit(bytes)?;
        if report.units.len() > budget {
          parser.seek()?;
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "coded frame budget exhausted",
          ));
        }
        let tasks = report
          .units
          .into_iter()
          .map(|unit| Task {
            payload: unit.payload,
            descriptor: Descriptor {
              geometry: Some(unit.geometry),
              plan: unit.plan,
              timestamp,
              source,
            },
          })
          .collect();
        Ok(Admission { tasks, end: report.end })
      }
      Codec::Framed(validator) => {
        if bytes.len() > rkmpp::MAX_PACKET_SIZE {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compressed input exceeds limit",
          ));
        }
        let ranges = validator.validate(bytes, budget)?;
        if ranges.is_empty() {
          self.metadata.append(bytes)?;
        }
        let mut tasks = Vec::new();
        for unit in ranges {
          self.serial =
            self.serial.checked_add(1).ok_or_else(|| {
              io::Error::other("coded picture identity exhausted")
            })?;
          // These codecs publish in presentation order. The synchronous
          // task may be hidden (no presentation); that does not
          // retain its submission metadata after task completion.
          // Hardware reference leases belong to
          // MPP, independently of this zero-reorder display policy.
          let plan = Plan::new(
            Id::new(self.serial),
            Part::Frame,
            Vec::new(),
            Limits::new(1, 0, None)?,
            false,
            Output { display: unit.display, ..Output::default() },
          );
          let configuration = if unit.configures_decoder {
            &[][..]
          } else {
            self.configuration.as_slice()
          };
          let payload =
            self.metadata.compose(configuration, bytes, unit.range)?;
          self.configuration.clear();
          tasks.push(Task {
            payload,
            descriptor: Descriptor {
              geometry: None,
              plan,
              timestamp,
              source,
            },
          });
        }
        Ok(Admission { tasks, end: false })
      }
    }
  }

  pub(crate) fn seek(&mut self) -> io::Result<()> {
    match &mut self.codec {
      Codec::Annexb(parser) => parser.seek(),
      Codec::Framed(validator) => {
        let configuration = validator.configuration();
        let mut validator = codec_parser::Validator::new(self.fourcc)?;
        if !configuration.is_empty() {
          validator.validate(&configuration, 0)?;
        }
        self.codec = Codec::Framed(validator);
        self.metadata.clear();
        self.configuration = configuration;
        Ok(())
      }
    }
  }
}
