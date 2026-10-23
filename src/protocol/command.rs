//! module: protocol::command; responsibility: admit complete typed
//! commands before resource registration. operations:
//! `TryFrom<Message>`, kind, bounded-value queries; entities: Command,
//! Route, Action, Stop, Slot, Capacity, Extent; tests: command_tests
//! and scheduler/process acceptance.
use std::io;
use std::num::NonZeroU32;

use rkmpp_bridge_uapi::wire::Flags;

use super::{Event, Message, VERSION};
use crate::codec::Codec;
use crate::timestamp::Source;
#[derive(Clone, Copy)]
pub struct Route {
  pub epoch: u64,
  pub generation: u32,
}
#[derive(Clone, Copy)]
pub struct Slot(usize);
impl Slot {
  pub fn get(self) -> usize {
    self.0
  }
}
#[derive(Clone, Copy)]
pub struct Capacity(NonZeroU32);
impl Capacity {
  pub fn get(self) -> usize {
    self.0.get() as usize
  }
}
#[derive(Clone, Copy)]
pub struct Extent {
  offset: usize,
  length: usize,
}
impl Extent {
  pub fn offset(self) -> usize {
    self.offset
  }

  pub fn length(self) -> usize {
    self.length
  }
}
#[derive(Clone, Copy)]
pub enum Stop {
  All,
  Capture { abort_drain: bool },
}
#[derive(Clone, Copy)]
pub enum Action {
  Packet {
    pool: NonZeroU32,
    index: Slot,
    token: NonZeroU32,
    extent: Extent,
    timestamp: u64,
    codec: Codec,
    source: Source,
  },
  Capture {
    pool: NonZeroU32,
    count: Capacity,
    reuse: bool,
  },
  Recycle {
    pool: NonZeroU32,
    index: Slot,
    token: NonZeroU32,
  },
  Drain,
  Reset {
    token: NonZeroU32,
    scope: Stop,
  },
  Close,
  Resume,
}
#[derive(Clone, Copy)]
pub struct Command {
  pub route: Route,
  pub action: Action,
}
fn invalid() -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, "invalid bridge command")
}
fn nonzero(value: u32) -> io::Result<NonZeroU32> {
  NonZeroU32::new(value).ok_or_else(invalid)
}
fn slot(value: u32) -> io::Result<Slot> {
  if value < super::registration::MAX_BUFFERS as u32 {
    Ok(Slot(value as usize))
  } else {
    Err(invalid())
  }
}
impl TryFrom<Message> for Command {
  type Error = io::Error;

  fn try_from(m: Message) -> io::Result<Self> {
    if m.version != VERSION {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unsupported protocol version",
      ));
    }
    if m.reserved != [0] {
      return Err(invalid());
    }
    if matches!(m.kind, value if value == Event::Recycle as u32
      || value == Event::Drain as u32
      || value == Event::Close as u32
      || value == Event::Resume as u32)
      && m.flags != 0
    {
      return Err(invalid());
    }
    let action = match m.kind {
      value if value == Event::Packet as u32 => {
        let offset = m.data_offset as usize;
        let length = m.bytesused as usize;
        if length == 0
          || offset
            .checked_add(length)
            .is_none_or(|end| end > rkmpp::MAX_PACKET_SIZE)
        {
          return Err(invalid());
        }
        Action::Packet {
          pool: nonzero(m.pool_id)?,
          index: slot(m.index)?,
          token: nonzero(m.count)?,
          extent: Extent { offset, length },
          timestamp: m.timestamp,
          codec: Codec::try_from(m.codec)?,
          source: Source::try_from(m.flags)?,
        }
      }
      value if value == Event::Capture as u32 => {
        if m.count > super::registration::MAX_BUFFERS as u32
          || m.flags & !(Flags::CaptureReuse as u32) != 0
        {
          return Err(invalid());
        }
        Action::Capture {
          pool: nonzero(m.pool_id)?,
          count: Capacity(nonzero(m.count)?),
          reuse: m.flags & Flags::CaptureReuse as u32 != 0,
        }
      }
      value if value == Event::Recycle as u32 => Action::Recycle {
        pool: nonzero(m.pool_id)?,
        index: slot(m.index)?,
        token: nonzero(m.count)?,
      },
      value if value == Event::Drain as u32 => Action::Drain,
      value if value == Event::Reset as u32 => {
        if m.flags
          & !(Flags::ResetCaptureOnly as u32
            | Flags::ResetAbortDrain as u32)
          != 0
          || m.flags & Flags::ResetAbortDrain as u32 != 0
            && m.flags & Flags::ResetCaptureOnly as u32 == 0
        {
          return Err(invalid());
        }
        Action::Reset {
          token: nonzero(m.count)?,
          scope: if m.flags & Flags::ResetCaptureOnly as u32 != 0 {
            Stop::Capture {
              abort_drain: m.flags & Flags::ResetAbortDrain as u32 != 0,
            }
          } else {
            Stop::All
          },
        }
      }
      value if value == Event::Close as u32 => Action::Close,
      value if value == Event::Resume as u32 => Action::Resume,
      _ => return Err(invalid()),
    };
    Ok(Self {
      route: Route { epoch: m.epoch, generation: m.generation },
      action,
    })
  }
}
impl Command {
  pub fn kind(&self) -> Event {
    match self.action {
      Action::Packet { .. } => Event::Packet,
      Action::Capture { .. } => Event::Capture,
      Action::Recycle { .. } => Event::Recycle,
      Action::Drain => Event::Drain,
      Action::Reset { .. } => Event::Reset,
      Action::Close => Event::Close,
      Action::Resume => Event::Resume,
    }
  }
}
