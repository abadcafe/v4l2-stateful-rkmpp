//! module: protocol::command; responsibility: admit wire events before
//! resources or session state can change. operations:
//! `TryFrom<Message>`, kind, message; entities: Command; tests:
//! command_tests, service/scheduler_tests, service/test_protocol.
//! Queue-state constraints remain with the backend.
use std::io;

use super::{Event, Message, VERSION};

#[derive(Clone, Copy)]
pub struct Command {
  message: Message,
  kind: Event,
}
impl TryFrom<Message> for Command {
  type Error = io::Error;

  fn try_from(message: Message) -> io::Result<Self> {
    if message.version != VERSION {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unsupported protocol version",
      ));
    }
    if message.reserved != [0] {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "nonzero event reserved field",
      ));
    }
    let kind = match message.kind {
      1 => Event::Packet,
      2 => Event::Capture,
      3 => Event::Recycle,
      4 => Event::Drain,
      5 => Event::Reset,
      6 => Event::Close,
      7 => Event::Resume,
      _ => {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "unknown bridge event",
        ));
      }
    };
    Ok(Self { message, kind })
  }
}
impl Command {
  pub fn kind(&self) -> Event {
    self.kind
  }

  pub fn message(&self) -> &Message {
    &self.message
  }
}
