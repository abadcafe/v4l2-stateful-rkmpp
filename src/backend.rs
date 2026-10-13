//! module: backend; responsibility: execute one owned streaming decode
//! session. operations: new, register, event, step, take_reply;
//! entities: Backend, stream state. tests: backend_tests and public
//! V4L2 codec/pixel/source-change/recovery tests.
use std::collections::VecDeque;
use std::io;

use crate::converter::{Converter, Input, Job};
use crate::image::Plane;
use crate::mpp::{Decoder, Frame, FrameInfo, Packet};
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::{Event, Message, Reply, VERSION};
mod capture;
#[cfg(test)]
mod capture_tests;
mod input;
#[cfg(test)]
mod input_tests;
use capture::CapturePool;
use input::InputPool;
enum Queued {
  Input(usize),
  Eos(Packet),
}
struct FrameInput(Frame);
impl Input for FrameInput {
  fn plane(&self) -> Plane<'_> {
    self
      .0
      .plane()
      .expect("validated frame DMA-BUF")
      .expect("decoded frame has a buffer")
  }
}
#[derive(Default)]
struct State {
  epoch: u64,
  generation: u32,
  expected: usize,
  info_wait: bool,
  ready: bool,
  ten: bool,
  eos_submitted: bool,
  changing: bool,
  converting: usize,
}
pub struct Backend {
  notify: Notify,
  decoder: Option<Decoder>,
  converter: Option<Converter<FrameInput>>,
  inputs: InputPool,
  capture: CapturePool,
  packets: VecDeque<Queued>,
  pending: Option<Frame>,
  replies: VecDeque<Message>,
  layout: Message,
  state: State,
}
impl Backend {
  pub fn new() -> io::Result<Self> {
    Ok(Self {
      notify: Notify::new()?,
      decoder: None,
      converter: None,
      inputs: InputPool::default(),
      capture: CapturePool::default(),
      packets: VecDeque::with_capacity(64),
      pending: None,
      replies: VecDeque::with_capacity(64),
      layout: Message::default(),
      state: State::default(),
    })
  }

  pub fn notification(&self) -> Notify {
    self.notify.clone()
  }

  fn emit(&mut self, mut message: Message) -> io::Result<()> {
    if self.replies.len() == 64 {
      return Err(io::Error::other("backend reply queue is full"));
    }
    message.pool_id = if message.kind == Reply::InputDone as u32 {
      self.inputs.id()
    } else {
      self.capture.id()
    };
    message.version = VERSION;
    message.epoch = self.state.epoch;
    message.generation = self.state.generation;
    self.replies.push_back(message);
    Ok(())
  }

  fn stop_conversions(&mut self) {
    self.converter = None;
    self.state.converting = 0;
  }

  fn reset(&mut self) -> io::Result<()> {
    self.stop_conversions();
    self.pending = None;
    self.capture = CapturePool::default();
    let result = if let Some(decoder) = self.decoder.take() {
      decoder.close()
    } else {
      Ok(())
    };
    self.packets.clear();
    self.inputs = InputPool::default();
    self.state.expected = 0;
    self.state.ready = false;
    self.state.info_wait = false;
    self.state.eos_submitted = false;
    result
  }

  fn replace_capture(&mut self) -> io::Result<()> {
    self.stop_conversions();
    self.capture = CapturePool::default();
    self.state.expected = 0;
    self.state.ready = false;
    self.state.changing = false;
    self
      .decoder
      .as_mut()
      .ok_or_else(|| {
        io::Error::other("capture reset before decoder initialization")
      })?
      .configure_output(self.state.ten)
  }

  pub fn register(&mut self, pool: Pool) -> io::Result<()> {
    let (kind, id, buffers) = pool.into_parts();
    if kind == Direction::Output {
      if !self.packets.is_empty() {
        return Err(io::Error::other(
          "input pool replacement with queued packets",
        ));
      }
      self.inputs = InputPool::new(id, buffers)?;
    } else if kind == Direction::Capture {
      if self.capture.id() != 0
        || !self.decoder.as_ref().is_some_and(Decoder::has_output)
      {
        return Err(io::Error::other(
          "unexpected capture registration",
        ));
      }
      if !self.state.ten {
        self.decoder.as_mut().unwrap().import_capture(&buffers)?;
      }
      self.capture = CapturePool::new(id, buffers);
    } else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown pool direction",
      ));
    }
    Ok(())
  }

  fn ready(&mut self) -> io::Result<()> {
    if !self.state.ready
            // Old-pool recycles during SOURCE_CHANGE cannot acknowledge the new layout.
            && !self.state.changing
            && self.state.info_wait
            && self.state.expected != 0
            && self.capture.len() >= self.state.expected
    {
      self.decoder.as_mut().unwrap().ready()?;
      self.state.ready = true;
      self.state.info_wait = false;
    }
    Ok(())
  }

  pub fn event(&mut self, message: Message) -> io::Result<()> {
    if message.version != VERSION {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "unsupported protocol version",
      ));
    }
    self.state.epoch = message.epoch;
    self.state.generation = message.generation;
    match message.kind {
      kind if kind == Event::Packet as u32 => {
        if message.pool_id == 0
          || message.pool_id != self.inputs.id()
          || self.packets.len() == 64
        {
          return Err(io::Error::other(
            "invalid packet pool or queue full",
          ));
        }
        if self.decoder.is_none() {
          self.decoder =
            Some(Decoder::new(message.codec, self.notify.clone())?);
        }
        self.inputs.prepare(
          message.index as usize,
          message.bytesused as usize,
          message.timestamp,
        )?;
        self.packets.push_back(Queued::Input(message.index as usize));
        Ok(())
      }
      kind if kind == Event::Capture as u32 => {
        self.state.expected = message.count as usize;
        self.ready()
      }
      kind if kind == Event::Recycle as u32 => {
        if message.pool_id == 0 || message.pool_id != self.capture.id()
        {
          return Err(io::Error::other("invalid recycle pool"));
        }
        self
          .capture
          .recycle(message.index as usize, self.state.changing)?;
        self.ready()
      }
      kind if kind == Event::Drain as u32 => {
        if self.decoder.is_none() || self.packets.len() == 64 {
          return Err(io::Error::other(
            "drain before initialization or packet queue full",
          ));
        }
        self.packets.push_back(Queued::Eos(Packet::eos()?));
        self.state.eos_submitted = true;
        Ok(())
      }
      kind if kind == Event::Reset as u32 => {
        if message.flags & Message::RESET_CAPTURE_ONLY != 0 {
          self.replace_capture()?;
        } else {
          self.reset()?;
        }
        self.replies.clear();
        self.emit(Message {
          kind: Reply::Stopped as u32,
          ..Default::default()
        })
      }
      kind if kind == Event::Resume as u32 => {
        if self.state.eos_submitted {
          if let Some(decoder) = &mut self.decoder {
            decoder.reset()?;
          }
          self.state.eos_submitted = false;
        }
        Ok(())
      }
      kind if kind == Event::Close as u32 => Ok(()),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown bridge event",
      )),
    }
  }

  fn announce(&mut self, info: FrameInfo) -> io::Result<()> {
    if info.format != 0 && info.format != 1 {
      return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "MPP output is not linear NV12/NV15",
      ));
    }
    self.state.changing = self.decoder.as_ref().unwrap().has_output();
    self.state.ready = false;
    self.state.ten = info.format == 1;
    let stride = if self.state.ten {
      info
        .width
        .checked_add(63)
        .ok_or_else(|| io::Error::other("frame width overflow"))?
        & !63
    } else {
      info.stride
    };
    let size = if self.state.ten {
      (stride as usize)
        .checked_mul(info.rows as usize)
        .and_then(|n| n.checked_mul(3))
        .map(|n| n / 2)
        .ok_or_else(|| io::Error::other("frame size overflow"))?
    } else {
      info.size
    };
    self.layout = Message {
      kind: Reply::Layout as u32,
      width: info.width,
      height: info.height,
      stride,
      rows: info.rows,
      size: u32::try_from(size).map_err(|_| {
        io::Error::other("frame buffer exceeds protocol size")
      })?,
      format: 1,
      colorspace: match info.primaries {
        9 => 10,
        1 => 3,
        5 | 6 => 1,
        _ => 0,
      },
      ycbcr_enc: match info.matrix {
        9 => 6,
        10 => 7,
        1 => 2,
        5 | 6 => 1,
        _ => 0,
      },
      xfer_func: match info.transfer {
        16 => 7,
        1 | 6 | 14 | 15 => 1,
        _ => 0,
      },
      quantization: match info.range {
        2 => 1,
        1 => 2,
        _ => 0,
      },
      ..Default::default()
    };
    if !self.state.changing {
      self
        .decoder
        .as_mut()
        .unwrap()
        .configure_output(self.state.ten)?;
    }
    self.state.info_wait = true;
    self.emit(self.layout)
  }

  fn frame_reply(
    &mut self,
    index: usize,
    info: FrameInfo,
  ) -> io::Result<()> {
    self.emit(Message {
      kind: Reply::Frame as u32,
      index: index as u32,
      bytesused: self.layout.stride * self.layout.rows * 3 / 2,
      timestamp: info.timestamp,
      ..Default::default()
    })?;
    if info.eos {
      self.emit(Message {
        kind: Reply::Eos as u32,
        ..Default::default()
      })?;
    }
    Ok(())
  }

  fn deliver(&mut self, frame: Frame) -> io::Result<Option<Frame>> {
    let info = frame.info();
    if info.failed {
      return Err(io::Error::other(
        "MPP reported a damaged or discarded frame",
      ));
    }
    if frame.plane()?.is_some() {
      if self.state.ten {
        let Some(index) = self.capture.next() else {
          return Ok(Some(frame));
        };
        if self.converter.is_none() {
          self.converter = Some(Converter::new(self.notify.clone())?);
        }
        let job = Job::new(
          index as u32,
          (self.layout.width, self.layout.height),
          FrameInput(frame),
          self.capture.image(
            index,
            self.layout.stride,
            self.layout.rows,
          )?,
        );
        if let Err(error) = self.converter.as_mut().unwrap().submit(job)
        {
          if !error.is_full() {
            return Err(io::Error::other("conversion worker stopped"));
          }
          return Ok(Some(error.into_job().into_input().0));
        }
        self.capture.hold(index, None)?;
        self.state.converting += 1;
        return Ok(None);
      }
      let index = frame.pool_index().ok_or_else(|| {
        io::Error::other("decoded frame has no capture index")
      })?;
      self.capture.hold(index, Some(frame))?;
      self.frame_reply(index, info)?;
    } else if info.eos {
      self.emit(Message {
        kind: Reply::Eos as u32,
        ..Default::default()
      })?;
    }
    Ok(None)
  }

  pub fn step(&mut self) -> io::Result<bool> {
    if self.decoder.is_none() {
      return Ok(false);
    }
    let mut progress = false;
    while let Some(completed) =
      self.converter.as_mut().and_then(Converter::take)
    {
      let (index, input, result) = completed.into_parts();
      result?;
      self.frame_reply(index as usize, input.0.info())?;
      drop(input);
      self.state.converting -= 1;
      progress = true;
    }
    if let Some(queued) = self.packets.front() {
      let packet = match queued {
        Queued::Input(index) => self.inputs.packet(*index)?,
        Queued::Eos(packet) => packet,
      };
      if self.decoder.as_mut().unwrap().packet(packet)? {
        if let Queued::Input(index) = self.packets.pop_front().unwrap()
        {
          self.inputs.complete(index)?;
          self.emit(Message {
            kind: Reply::InputDone as u32,
            index: index as u32,
            ..Default::default()
          })?;
        }
        progress = true;
      }
    }
    if self.replies.len() > 58 {
      return Ok(progress);
    }
    let frame = match self.pending.take() {
      Some(frame) => Some(frame),
      None => self.decoder.as_mut().unwrap().frame()?,
    };
    let Some(frame) = frame else {
      return Ok(progress);
    };
    let info = frame.info();
    if self.state.converting != 0
      && (info.info_change || frame.plane()?.is_none())
    {
      self.pending = Some(frame);
      return Ok(progress);
    }
    if info.info_change {
      self.announce(info)?;
      return Ok(true);
    }
    self.pending = self.deliver(frame)?;
    Ok(progress || self.pending.is_none())
  }

  pub fn take_reply(&mut self) -> Option<Message> {
    self.replies.pop_front()
  }
}
impl Drop for Backend {
  fn drop(&mut self) {
    if let Err(error) = self.reset() {
      eprintln!("decoder teardown: {error}");
    }
  }
}
