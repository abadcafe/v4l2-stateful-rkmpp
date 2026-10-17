//! module: backend; responsibility: execute one owned streaming decode
//! session. operations: new, register, event, step, take_reply;
//! entities: Backend, stream state. tests: backend_tests and public
//! V4L2 codec/pixel/source-change/recovery tests.
use std::collections::VecDeque;
use std::io;

use crate::frontend::{Frontend, Payload};
use crate::mpp::{Decoder, Packet};
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{Event, Message, Reply, VERSION};
use crate::submission::Origin;
mod presentation;
#[cfg(test)]
mod presentation_tests;
use presentation::Presentation;
mod color;
#[cfg(test)]
mod color_tests;
mod layout;
#[cfg(test)]
mod layout_tests;
use layout::{Extent, Layouts};
mod capture;
#[cfg(test)]
mod capture_tests;
mod frame;
mod input;
#[cfg(test)]
mod input_tests;
mod output;
#[cfg(test)]
mod output_tests;
use frame::FrameInput;
use input::InputPool;
use output::{Output, Status};
enum Queued {
  Analyze {
    index: usize,
    timestamp: u64,
    source: crate::timestamp::Source,
    count: u32,
    origin: Origin,
  },
  Task {
    packet: Packet,
    serial: u64,
    plan: crate::picture::Plan,
  },
  Release(usize),
  Eos(Option<u32>),
}
mod flow;
#[cfg(test)]
mod flow_tests;
use flow::{Completion, End, Flow};
#[derive(Default)]
struct State {
  epoch: u64,
  codec: u32,
  generation: u32,
  flow: Flow,
}
pub struct Backend {
  notify: Notify,
  decoder: Option<Decoder>,
  frontend: Option<Frontend>,
  layouts: Layouts,
  completions: VecDeque<(usize, Option<Message>)>,
  inputs: InputPool,
  output: Output,
  packets: VecDeque<Queued>,
  active_task: Option<u64>,
  pending: Option<FrameInput>,
  presentation: Presentation<FrameInput>,
  replies: VecDeque<Message>,
  state: State,
}
impl Backend {
  pub fn new() -> io::Result<Self> {
    let notify = Notify::new()?;
    Ok(Self {
      output: Output::new(notify.clone()),
      notify,
      decoder: None,
      frontend: None,
      layouts: Layouts::default(),
      completions: VecDeque::new(),
      inputs: InputPool::default(),
      packets: VecDeque::with_capacity(64),
      active_task: None,
      pending: None,
      presentation: Presentation::default(),
      replies: VecDeque::with_capacity(64),
      state: State::default(),
    })
  }

  pub fn notification(&self) -> Notify {
    self.notify.clone()
  }

  fn emit(&mut self, mut message: Message) -> io::Result<()> {
    if message.kind == Reply::InputDone as u32 {
      let entry = self
        .completions
        .iter_mut()
        .find(|(index, _)| *index == message.index as usize)
        .ok_or_else(|| {
          io::Error::other("completion without queued input")
        })?;
      if entry.1.replace(message).is_some() {
        return Err(io::Error::other("duplicate input completion"));
      }
      while self
        .completions
        .front()
        .is_some_and(|(_, done)| done.is_some())
      {
        let (index, done) = self.completions.pop_front().unwrap();
        let damaged = self.inputs.complete(index)?;
        let mut done = done.unwrap();
        if damaged {
          done.flags |= Message::FRAME_ERROR;
        }
        done.pool_id = self.inputs.id();
        done.version = VERSION;
        done.epoch = self.state.epoch;
        done.generation = self.state.generation;
        self.replies.push_back(done);
      }
      return Ok(());
    }
    if self.replies.len() == 64 {
      return Err(io::Error::other("backend reply queue is full"));
    }
    message.pool_id = self.output.id();
    message.version = VERSION;
    message.epoch = self.state.epoch;
    message.generation = self.state.generation;
    self.replies.push_back(message);
    Ok(())
  }

  fn reset(&mut self) -> io::Result<()> {
    self.pending = None;
    self.presentation = Presentation::default();
    self.output = Output::new(self.notify.clone());
    let result = if let Some(decoder) = self.decoder.take() {
      decoder.close()
    } else {
      Ok(())
    };
    if let Some(parser) = self.frontend.as_mut() {
      parser.seek()?;
    }
    self.layouts = Layouts::default();
    self.completions.clear();
    self.packets.clear();
    self.active_task = None;
    self.inputs = InputPool::default();
    self.state.flow = Flow::default();
    result
  }

  fn abort_drain(&mut self) -> io::Result<()> {
    if self.state.flow.running()
      && let Some(index) =
        self.packets.iter().position(|p| matches!(p, Queued::Eos(..)))
    {
      self.packets.remove(index);
    }
    if self.state.flow.abort() {
      self.resume_natural()?;
    }
    self.replies.retain(|m| {
      m.kind != Reply::Eos as u32 && m.kind != Reply::Draining as u32
    });
    Ok(())
  }

  pub fn register(&mut self, pool: Pool) -> io::Result<()> {
    let (kind, id, buffers) = pool.into_parts();
    if kind == Direction::Output {
      if !self.packets.is_empty() || self.active_task.is_some() {
        return Err(io::Error::other(
          "input pool replacement with queued packets",
        ));
      }
      self.inputs = InputPool::new(id, buffers)?;
    } else if kind == Direction::Capture {
      self.output.register(id, buffers, self.decoder.as_ref())?;
    } else {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown pool direction",
      ));
    }
    Ok(())
  }

  pub fn event(&mut self, command: Command) -> io::Result<()> {
    let message = command.message();
    self.state.epoch = message.epoch;
    self.state.generation = message.generation;
    match command.kind() {
      Event::Packet => {
        if message.pool_id == 0 || message.pool_id != self.inputs.id() {
          return Err(io::Error::other("invalid packet pool"));
        }
        if self.decoder.is_none() {
          if self.frontend.is_none()
            || self.state.codec != message.codec
          {
            self.frontend = Some(Frontend::new(message.codec)?);
          }
          self.state.codec = message.codec;
          self.decoder =
            Some(Decoder::new(message.codec, self.notify.clone())?);
        }
        let origin = self.inputs.prepare(
          message.index as usize,
          message.data_offset as usize,
          message.bytesused as usize,
          message.timestamp,
          crate::timestamp::Source::try_from(message.flags & 0x70000)?,
        )?;
        self.completions.push_back((message.index as usize, None));
        self.packets.push_back(Queued::Analyze {
          index: message.index as usize,
          timestamp: message.timestamp,
          source: crate::timestamp::Source::try_from(
            message.flags & 0x70000,
          )?,
          count: message.count,
          origin,
        });
        Ok(())
      }
      Event::Capture => self.output.start(
        message.count as usize,
        message.flags & Message::CAPTURE_REUSE != 0,
        self.decoder.as_mut(),
      ),
      Event::Recycle => self.output.recycle(
        message.pool_id,
        message.index as usize,
        self.decoder.as_mut(),
      ),
      Event::Drain => {
        if self.decoder.is_none() {
          return self.emit(Message {
            kind: Reply::Eos as u32,
            ..Default::default()
          });
        }
        self.packets.push_back(Queued::Eos(None));
        Ok(())
      }
      Event::Reset => {
        if message.flags & Message::RESET_CAPTURE_ONLY != 0 {
          if message.flags & Message::RESET_ABORT_DRAIN != 0 {
            self.abort_drain()?;
          }
          self.output.stop(self.decoder.as_mut())?;
        } else {
          self.reset()?;
          self.replies.clear();
        }
        self.emit(Message {
          kind: Reply::Stopped as u32,
          ..Default::default()
        })
      }
      Event::Resume => {
        if self.state.flow.resume()? {
          self.resume_natural()?;
        }
        Ok(())
      }
      Event::Close => Ok(()),
    }
  }

  fn resume_natural(&mut self) -> io::Result<()> {
    let decoder = self.decoder.as_mut().ok_or_else(|| {
      io::Error::other("natural resume without decoder")
    })?;
    decoder.reset()?;
    self.active_task = None;
    Ok(())
  }

  fn finish_drain(&mut self) -> io::Result<()> {
    self.layouts.drained()?;
    match self.state.flow.complete()? {
      Completion::Notify => self.emit(Message {
        kind: Reply::Eos as u32,
        ..Default::default()
      }),
      Completion::Continue { reset } => {
        if reset {
          self.resume_natural()?;
        }
        Ok(())
      }
    }
  }

  fn parse_front(&mut self) -> io::Result<bool> {
    if !matches!(self.packets.front(), Some(Queued::Analyze { .. })) {
      return Ok(false);
    }
    let Queued::Analyze { index, timestamp, source, count, origin } =
      self.packets.pop_front().unwrap()
    else {
      unreachable!()
    };
    let admission = self.frontend.as_mut().unwrap().admit(
      self.inputs.bytes(index)?,
      timestamp,
      source,
    );
    let admission = match admission {
      Ok(admission) => admission,
      Err(error) if error.kind() == io::ErrorKind::InvalidData => {
        eprintln!("damaged input: {error}");
        self.inputs.damage(&[origin]);
        self.packets.push_front(Queued::Release(index));
        return Ok(true);
      }
      Err(error) => return Err(error),
    };
    let mut work = Vec::new();
    for task in admission.tasks {
      let serial = self.layouts.push(&task.descriptor)?;
      let packet = match task.payload {
        Payload::Input(range) => {
          self.inputs.task(index, range, serial, source)?
        }
        Payload::Owned(bytes) => Packet::owned(bytes, serial, source)?,
      };
      work.push(Queued::Task {
        packet,
        serial,
        plan: task.descriptor.plan,
      });
    }
    work.push(Queued::Release(index));
    if admission.end {
      work.push(Queued::Eos(Some(count)));
    }
    for item in work.into_iter().rev() {
      self.packets.push_front(item);
    }
    Ok(true)
  }

  pub fn step(&mut self) -> io::Result<bool> {
    if self.decoder.is_none() || self.output.status() == Status::Stopped
    {
      return Ok(false);
    }
    let mut progress = false;
    if self.state.flow.running() {
      progress |= self.parse_front()?;
    }
    while let Some(message) = self.output.take()? {
      self.emit(message)?;
      progress = true;
    }
    if self.state.flow.running()
      && let Some(Queued::Release(index)) = self.packets.front()
    {
      let index = *index;
      self.emit(Message {
        kind: Reply::InputDone as u32,
        index: index as u32,
        ..Default::default()
      })?;
      self.packets.pop_front();
      progress = true;
    }
    if self.decoder.as_ref().unwrap().idle()
      && let Some(serial) = self.active_task.take()
    {
      self.layouts.finish(serial);
    }
    if self.state.flow.running()
      && let Some(queued) = self.packets.front()
    {
      if self.decoder.as_ref().unwrap().idle()
        && let Queued::Task { plan, .. } = queued
      {
        progress |= self.presentation.prepare(plan);
      }
      let accepted = match queued {
        Queued::Release(_) | Queued::Analyze { .. } => return Ok(true),
        Queued::Task { packet, .. } => {
          let state = self.decoder.as_mut().unwrap().packet(packet)?;
          progress |= state != crate::mpp::Submission::Blocked;
          state == crate::mpp::Submission::Consumed
        }
        Queued::Eos(_) => self.decoder.as_ref().unwrap().idle(),
      };
      if accepted {
        match self.packets.pop_front().unwrap() {
          Queued::Task { serial, .. } => {
            self.active_task = Some(serial)
          }
          Queued::Eos(natural) => {
            self.presentation.drain();
            self.state.flow.begin(if natural.is_some() {
              End::Natural
            } else {
              End::Explicit
            })?;
            if let Some(boundary) = natural {
              self.emit(Message {
                kind: Reply::Draining as u32,
                count: boundary,
                ..Default::default()
              })?;
            }
          }
          _ => unreachable!("checked decode task"),
        }
        progress = true;
      }
    }
    if self.replies.len() > 58 {
      return Ok(progress);
    }
    if self.pending.is_none() {
      if let Some(frame) = self.decoder.as_mut().unwrap().frame()? {
        let mut info = frame.info();
        {
          if info.info_change {
            if self.layouts.discard_prior(info.timestamp)? {
              self.presentation.discard();
            }
            let extent = self.layouts.peek(
              info.timestamp,
              info.width,
              info.height,
            )?;
            self.presentation.barrier(FrameInput {
              frame,
              info,
              extent,
            });
          } else if let Some(picture) = self
            .layouts
            .complete(&mut info, frame.plane()?.is_some())?
          {
            if picture.plan.output().before
              == crate::picture::Boundary::Discard
            {
              self.presentation.discard();
            } else if picture.plan.output().before
              == crate::picture::Boundary::Drain
            {
              self.presentation.drain();
            }
            self.presentation.picture(
              FrameInput {
                frame,
                info,
                extent: picture
                  .extent
                  .unwrap_or(Extent::from_frame(info)),
              },
              &picture.plan,
            );
            if picture.plan.output().drain_after {
              self.presentation.drain();
            }
          } else {
            let extent =
              self.output.extent().unwrap_or(Extent::from_frame(info));
            self.presentation.barrier(FrameInput {
              frame,
              info,
              extent,
            });
          }
        }
        progress = true;
      }
      if self.pending.is_none() {
        self.pending = self.presentation.take();
      }
    }
    let Some(frame) = self.pending.take() else {
      if self.state.flow.waiting()
        && self.decoder.as_ref().unwrap().idle()
        && self.output.idle()
      {
        self.finish_drain()?;
        progress = true;
      }
      return Ok(progress);
    };
    let info = frame.info;
    if frame.frame.plane()?.is_none()
      && info.eos
      && self.output.idle()
      && self.state.flow.waiting()
    {
      self.finish_drain()?;
      return Ok(true);
    }
    let (pending, reply) =
      self.output.push(frame, self.decoder.as_mut().unwrap())?;
    self.pending = pending;
    if let Some(message) = reply {
      self.emit(message)?;
      progress = true;
    }
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
