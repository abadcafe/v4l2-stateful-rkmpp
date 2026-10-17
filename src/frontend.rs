//! module: frontend; responsibility: turn framed codec input into
//! uniform decode tasks. operations: new, admit, seek; entities:
//! Frontend, Codec, Admission, Task, Payload; tests: frontend_tests.
use std::io;

use crate::picture::{Descriptor, Id, Limits, Output, Part, Plan};
pub(crate) use crate::submission::Payload;
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
  Framed(crate::packet_syntax::Validator),
}
pub(crate) struct Frontend {
  codec: Codec,
  fourcc: u32,
  serial: u64,
  metadata: Vec<u8>,
}
impl Frontend {
  pub(crate) fn new(fourcc: u32) -> io::Result<Self> {
    let codec = match &fourcc.to_le_bytes() {
      b"H264" | b"HEVC" => {
        Codec::Annexb(crate::bitstream::Parser::new(fourcc)?)
      }
      _ => Codec::Framed(crate::packet_syntax::Validator::new(fourcc)?),
    };
    Ok(Self { codec, fourcc, serial: 0, metadata: Vec::new() })
  }

  pub(crate) fn admit(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
  ) -> io::Result<Admission> {
    let admission = self.inspect(bytes, timestamp, source)?;
    if admission.tasks.iter().any(|task| {
      let size = match &task.payload {
        Payload::Input(range) => range.len(),
        Payload::Owned(bytes) => bytes.len(),
      };
      size > crate::submission::MAX_PACKET_SIZE
    }) {
      self.seek()?;
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "compressed task including metadata exceeds input limit",
      ));
    }
    Ok(admission)
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    timestamp: u64,
    source: Source,
  ) -> io::Result<Admission> {
    match &mut self.codec {
      Codec::Annexb(parser) => {
        let report = parser.admit(bytes)?;
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
        let ranges = validator.validate(bytes)?;
        if ranges.is_empty() {
          if self
            .metadata
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > crate::submission::MAX_PACKET_SIZE)
          {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "metadata prefix exceeds input limit",
            ));
          }
          self.metadata.extend_from_slice(bytes);
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
          let payload = if self.metadata.is_empty() {
            Payload::Input(unit.range)
          } else {
            let mut prefix = std::mem::take(&mut self.metadata);
            prefix.extend_from_slice(&bytes[unit.range]);
            Payload::Owned(prefix)
          };
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
        let mut next = Self::new(self.fourcc)?;
        if !configuration.is_empty() {
          next.admit(&configuration, 0, Source::End)?;
        }
        *self = next;
        Ok(())
      }
    }
  }
}
