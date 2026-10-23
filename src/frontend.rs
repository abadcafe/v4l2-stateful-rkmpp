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
  frames: Framed,
}
#[derive(Default)]
struct Framed {
  serial: u64,
  metadata: crate::submission::Prefix,
  configuration: codec_parser::Configuration,
}
impl Frontend {
  pub(crate) fn new(fourcc: u32) -> io::Result<Self> {
    let codec = match &fourcc.to_le_bytes() {
      b"H264" | b"HEVC" => {
        Codec::Annexb(crate::bitstream::Parser::new(fourcc)?)
      }
      _ => Codec::Framed(codec_parser::Validator::new(fourcc)?),
    };
    Ok(Self { codec, frames: Framed::default() })
  }

  pub(crate) fn admit(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
    budget: usize,
  ) -> io::Result<Admission> {
    let budget = budget.min(32);
    match &mut self.codec {
      Codec::Annexb(parser) => {
        let report = parser.admit(bytes, budget)?;
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
          self.seek()?;
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compressed input exceeds limit",
          ));
        }
        let result = validator.admit(bytes, budget, |_, ranges| {
          self.frames.compose(bytes, timestamp, source, ranges)
        });
        if result.is_err() {
          self.frames.metadata.clear();
          self.frames.configuration = validator.configuration();
        }
        result
      }
    }
  }

  pub(crate) fn seek(&mut self) -> io::Result<()> {
    match &mut self.codec {
      Codec::Annexb(parser) => parser.seek(),
      Codec::Framed(validator) => {
        validator.reset()?;
        let configuration = validator.configuration();
        self.frames.metadata.clear();
        self.frames.configuration = configuration;
        Ok(())
      }
    }
  }
}

impl Framed {
  fn compose(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
    ranges: Vec<codec_parser::unit::Unit>,
  ) -> io::Result<Admission> {
    if ranges.is_empty() {
      self.metadata.append(bytes)?;
    }
    let mut tasks = Vec::new();
    for unit in ranges {
      self.serial = self.serial.checked_add(1).ok_or_else(|| {
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
      self.configuration = codec_parser::Configuration::default();
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
