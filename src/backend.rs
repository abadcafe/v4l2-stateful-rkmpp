//! module: backend; responsibility: execute one owned streaming decode
//! session. operations: new, register, event, step, take_reply;
//! entities: Backend, stream state. tests: backend_tests and public
//! V4L2 codec/pixel/source-change/recovery tests.
use std::collections::VecDeque;
use std::io;
use std::num::NonZeroU32;

use rkmpp::{Decoder, Packet};
use rkmpp_bridge_uapi::wire::Flags;

use crate::frontend::Frontend;
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::{Action, Command, Stop};
use crate::protocol::{Message, Reply, VERSION};
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
use layout::{Layouts, Requirements};
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
    token: NonZeroU32,
    origin: Origin,
  },
  Task {
    packet: Packet,
    serial: u64,
  },
  Release(usize, NonZeroU32),
  Eos(Option<u32>),
}
mod flow;
#[cfg(test)]
mod flow_tests;
use flow::{Completion, End, Flow};
#[derive(Default)]
struct State {
  epoch: u64,
  codec: Option<crate::codec::Codec>,
  generation: u32,
  flow: Flow,
}
pub struct Backend {
  factory: crate::quantize::Factory,
  notify: Notify,
  decoder: Option<Decoder>,
  frontend: Option<Frontend>,
  layouts: Layouts,
  inputs: InputPool,
  output: Output,
  packets: VecDeque<Queued>,
  prepared_tasks: Vec<Queued>,
  active_task: Option<u64>,
  pending: Option<FrameInput>,
  presentation: Presentation<FrameInput>,
  replies: VecDeque<Message>,
  state: State,
}
impl Backend {
  pub fn new(factory: crate::quantize::Factory) -> io::Result<Self> {
    let notify = Notify::new()?;
    Ok(Self {
      output: Output::new(factory.clone()),
      factory,
      notify,
      decoder: None,
      frontend: None,
      layouts: Layouts::default(),
      inputs: InputPool::default(),
      packets: VecDeque::with_capacity(64),
      prepared_tasks: Vec::with_capacity(4),
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
    if self.replies.len() == 64 {
      return Err(io::Error::other("backend reply queue is full"));
    }
    message.pool_id = if message.kind == Reply::InputDone as u32 {
      // Release is executed only at the head of the input queue, after
      // all of this input's synchronous decode tasks. InputPool owns
      // the lease; no second completion/order ledger is needed.
      if self.inputs.complete(message.index as usize)? {
        message.flags |= Flags::FrameError as u32;
      }
      self.inputs.id()
    } else {
      self.output.id()
    };
    message.version = VERSION;
    message.epoch = self.state.epoch;
    message.generation = self.state.generation;
    self.replies.push_back(message);
    Ok(())
  }

  fn reset(&mut self) -> io::Result<()> {
    self.pending = None;
    self.presentation = Presentation::default();
    self.output = Output::new(self.factory.clone());
    let result = if let Some(decoder) = self.decoder.take() {
      decoder.close()
    } else {
      Ok(())
    };
    if let Some(parser) = self.frontend.as_mut() {
      parser.seek()?;
    }
    self.layouts = Layouts::default();
    self.packets.clear();
    self.prepared_tasks.clear();
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
    match kind {
      Direction::Output => {
        if !self.packets.is_empty() || self.active_task.is_some() {
          return Err(io::Error::other(
            "input pool replacement with queued packets",
          ));
        }
        self.inputs = InputPool::new(id, buffers)?;
      }
      Direction::Capture => {
        self.output.register(id, buffers, self.decoder.as_ref())?;
      }
    }
    Ok(())
  }

  pub fn event(&mut self, command: Command) -> io::Result<()> {
    self.state.epoch = command.route.epoch;
    self.state.generation = command.route.generation;
    match command.action {
      Action::Packet {
        pool,
        index,
        token,
        extent,
        timestamp,
        codec,
        source,
      } => {
        if pool.get() != self.inputs.id() {
          return Err(io::Error::other("invalid packet pool"));
        }
        if self.decoder.is_some() && self.state.codec != Some(codec) {
          return Err(io::Error::other(
            "compressed format changed without reset",
          ));
        }
        if self.decoder.is_none() {
          if self.frontend.is_none() || self.state.codec != Some(codec)
          {
            self.frontend = Some(Frontend::new(codec.fourcc())?);
          }
          self.state.codec = Some(codec);
          self.decoder = Some(Decoder::new(codec.fourcc(), {
            let notify = self.notify.clone();
            move || notify.signal()
          })?);
        }
        let origin = self.inputs.prepare(
          index.get(),
          extent.offset(),
          extent.length(),
        )?;
        self.packets.push_back(Queued::Analyze {
          index: index.get(),
          timestamp,
          source,
          token,
          origin,
        });
        Ok(())
      }
      Action::Capture { count, reuse, .. } => {
        self.output.start(count.get(), reuse, self.decoder.as_mut())
      }
      Action::Recycle { pool, index, token } => self.output.recycle(
        pool.get(),
        index.get(),
        token,
        self.decoder.as_mut(),
      ),
      Action::Drain => {
        if self.decoder.is_none() {
          return self.emit(Message {
            kind: Reply::Eos as u32,
            ..Default::default()
          });
        }
        self.packets.push_back(Queued::Eos(None));
        Ok(())
      }
      Action::Reset { token, scope } => {
        if let Stop::Capture { abort_drain } = scope {
          if abort_drain {
            self.abort_drain()?;
          }
          self.output.stop(self.decoder.as_mut())?;
        } else {
          self.reset()?;
          self.replies.clear();
        }
        self.emit(Message {
          kind: Reply::Stopped as u32,
          count: token.get(),
          ..Default::default()
        })
      }
      Action::Resume => {
        if self.state.flow.resume()? {
          self.resume_natural()?;
        }
        Ok(())
      }
      Action::Close => Ok(()),
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
    let Queued::Analyze { index, timestamp, source, token, origin } =
      self.packets.pop_front().unwrap()
    else {
      unreachable!()
    };
    let admission = self.frontend.as_mut().unwrap().admit(
      self.inputs.bytes(index)?,
      timestamp,
      source,
      self.layouts.budget(),
    );
    let admission = match admission {
      Ok(admission) => admission,
      Err(error) if error.kind() == io::ErrorKind::InvalidData => {
        eprintln!("damaged input: {error}");
        self.inputs.damage(&[origin]);
        self.packets.push_front(Queued::Release(index, token));
        return Ok(true);
      }
      Err(error) => return Err(error),
    };
    debug_assert!(self.prepared_tasks.is_empty());
    for task in admission.tasks {
      let serial = self.layouts.push(task.descriptor)?;
      let packet =
        self.inputs.task(index, task.payload, serial, source)?;
      self.prepared_tasks.push(Queued::Task { packet, serial });
    }
    self.prepared_tasks.push(Queued::Release(index, token));
    if admission.end {
      self.prepared_tasks.push(Queued::Eos(Some(token.get())));
    }
    for item in self.prepared_tasks.drain(..).rev() {
      self.packets.push_front(item);
    }
    Ok(true)
  }

  fn advance_input(&mut self) -> io::Result<bool> {
    if self.decoder.as_ref().unwrap().idle()
      && let Some(serial) = self.active_task.take()
    {
      self.layouts.finish(serial);
    }
    let mut progress = false;
    if self.state.flow.running()
      && let Some(queued) = self.packets.front()
    {
      let accepted = match queued {
        Queued::Release(..) | Queued::Analyze { .. } => {
          return Ok(true);
        }
        Queued::Task { packet, .. } => {
          let state = self.decoder.as_mut().unwrap().packet(packet)?;
          progress |= state != rkmpp::Submission::Blocked;
          state == rkmpp::Submission::Consumed
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
    Ok(progress)
  }

  pub fn step(&mut self) -> io::Result<bool> {
    if self.decoder.is_none() || self.output.status() == Status::Stopped
    {
      return Ok(false);
    }
    let mut progress = false;
    while let Some(message) = self.output.take()? {
      self.emit(message)?;
      progress = true;
    }
    if self.state.flow.running() {
      progress |= self.parse_front()?;
    }

    if self.state.flow.running()
      && let Some(Queued::Release(index, token)) = self.packets.front()
    {
      let (index, token) = (*index, *token);
      self.emit(Message {
        kind: Reply::InputDone as u32,
        index: index as u32,
        count: token.get(),
        ..Default::default()
      })?;
      self.packets.pop_front();
      progress = true;
    }
    if self.state.flow.running()
      && let Some(Queued::Task { serial, .. }) = self.packets.front()
      && self.decoder.as_ref().unwrap().idle()
    {
      progress |=
        self.presentation.prepare(self.layouts.plan(*serial)?);
    }
    if self.replies.len() > 58 {
      return Ok(progress);
    }
    if self.pending.is_none() {
      if let Some(frame) = self.decoder.as_mut().unwrap().frame()? {
        let mut info = frame.info();
        {
          if info.info_change {
            if self.layouts.discard_prior(info.pts as u64)? {
              self.presentation.discard();
            }
            let requirements = self.layouts.peek(
              info.pts as u64,
              info.width,
              info.height,
            )?;
            self.presentation.barrier(FrameInput {
              frame,
              info,
              requirements,
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
                requirements: picture
                  .requirements
                  .unwrap_or(Requirements::from_frame(info)),
              },
              &picture.plan,
            );
            if picture.plan.output().drain_after {
              self.presentation.drain();
            }
          } else {
            let requirements = self
              .output
              .requirements()
              .unwrap_or(Requirements::from_frame(info));
            self.presentation.barrier(FrameInput {
              frame,
              info,
              requirements,
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
      // A conversion retains its source and destination. Decode can
      // fill one pending picture concurrently; pending backpressure
      // then prevents admission of further work.
      if !progress {
        progress |= self.advance_input()?;
      }
      if !progress
        && self.state.flow.waiting()
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

  pub fn completion(&self) -> Option<crate::descriptor::Lease> {
    self.output.completion()
  }

  pub fn take_reply(&mut self) -> Option<Message> {
    self.replies.pop_front()
  }
}
impl Drop for Backend {
  fn drop(&mut self) {
    // Destruction retires GPU/display leases before the decoder. A seek
    // transaction would recreate a parser that has no future consumer.
    self.pending = None;
    self.presentation = Presentation::default();
    self.output = Output::new(self.factory.clone());
    if let Some(decoder) = self.decoder.take()
      && let Err(error) = decoder.close()
    {
      eprintln!("decoder teardown: {error}");
    }
  }
}
