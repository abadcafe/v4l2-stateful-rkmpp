//! module: service::worker; responsibility: schedule one bounded bridge
//! decode session. operations: run, Channel, Engine; entities:
//! dependency contracts and Statistics. tests: worker_tests. Dependency
//! traits belong to their scheduling consumer.
use std::collections::VecDeque;
use std::io;

use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{
  Batch, Event, MAX_BATCH, Message, Reply, VERSION,
};
use crate::shutdown::Shutdown;
pub(super) trait Channel {
  fn exchange(&mut self, batch: &mut Batch) -> io::Result<()>;
  fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<Pool>;
  fn wait(
    &mut self,
    notify: &Notify,
    shutdown: &Shutdown,
  ) -> io::Result<bool>;
}
pub(super) trait Engine {
  fn notification(&self) -> Notify;
  fn register(&mut self, pool: Pool) -> io::Result<()>;
  fn event(&mut self, command: Command) -> io::Result<()>;
  fn step(&mut self) -> io::Result<bool>;
  fn take_reply(&mut self) -> Option<Message>;
}
#[derive(Debug)]
pub(super) struct Statistics {
  pub exchanges: u64,
  pub registrations: u64,
  pub events: u64,
  pub replies: u64,
  pub max_events: u32,
  pub max_replies: u32,
}
pub(super) fn run(
  control: &mut impl Channel,
  mut backend: impl Engine,
  shutdown: &Shutdown,
) -> Result<Statistics, String> {
  let notify = backend.notification();
  let mut current = Message::default();
  let mut pools = [0u32; 2];
  let mut fetch = true;
  let (mut max_events, mut max_replies) = (0u32, 0u32);
  let mut pending = VecDeque::with_capacity(64);
  let (mut exchanges, mut registrations, mut events, mut replies) =
    (0u64, 0u64, 0u64, 0u64);
  let result = (|| {
    loop {
      if shutdown.stopping() {
        return Ok(());
      }
      let mut batch = Batch { version: VERSION, ..Default::default() };
      while (batch.reply_count as usize) < MAX_BATCH {
        let Some(reply) = pending.pop_front() else {
          break;
        };
        batch.replies[batch.reply_count as usize] = reply;
        batch.reply_count += 1;
      }
      if fetch || batch.reply_count != 0 {
        exchanges += 1;
        replies += u64::from(batch.reply_count);
        if let Err(error) = control.exchange(&mut batch) {
          return if error.raw_os_error() == Some(libc::EPIPE) {
            Ok(())
          } else {
            Err(format!("exchange: {error}"))
          };
        }
        max_events = max_events.max(batch.event_count);
        max_replies = max_replies.max(batch.reply_count);
      }
      fetch = batch.event_count as usize == MAX_BATCH;
      for status in &batch.results[..batch.reply_count as usize] {
        if *status < 0
          && *status != -libc::ESTALE
          && *status != -libc::EPIPE
        {
          return Err(format!(
            "reply: {}",
            io::Error::from_raw_os_error(-status)
          ));
        }
      }
      events += u64::from(batch.event_count);
      for message in &batch.events[..batch.event_count as usize] {
        current = *message;
        let command = Command::try_from(*message)
          .map_err(|error| error.to_string())?;
        if command.kind() == Event::Close {
          return Ok(());
        }
        let direction = match command.kind() {
          Event::Packet => Some((0, Direction::Output)),
          Event::Capture | Event::Recycle => {
            Some((1, Direction::Capture))
          }
          _ => None,
        };
        if let Some((slot, kind)) = direction
          && pools[slot] != message.pool_id
        {
          let pool = match control.register(
            kind,
            message.epoch,
            message.pool_id,
          ) {
            Ok(pool) => pool,
            Err(error)
              if matches!(
                error.raw_os_error(),
                Some(libc::EPIPE | libc::ESTALE)
              ) =>
            {
              continue;
            }
            Err(error) => {
              return Err(format!("register pool: {error}"));
            }
          };
          backend
            .register(pool)
            .map_err(|e| format!("backend pool registration: {e}"))?;
          registrations += 1;
          pools[slot] = message.pool_id;
        }
        backend.event(command).map_err(|e| {
          format!("backend event {}: {e}", message.kind)
        })?;
        if command.kind() == Event::Reset {
          if message.flags & Message::RESET_CAPTURE_ONLY != 0 {
            pools[1] = 0;
          } else {
            pools = [0; 2];
          }
        }
      }
      let mut progressed = false;
      for _ in 0..4 {
        if !backend
          .step()
          .map_err(|e| format!("backend progress: {e}"))?
        {
          break;
        }
        progressed = true;
      }
      while let Some(reply) = backend.take_reply() {
        pending.push_back(reply);
      }
      if pending.len() > 64 {
        return Err("reply backpressure overflow".into());
      }
      if !pending.is_empty()
        || fetch
        || batch.event_count != 0
        || progressed
      {
        if progressed {
          fetch = true;
        }
        continue;
      }
      match control.wait(&notify, shutdown) {
        Ok(control_ready) => fetch = control_ready,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
        Err(error) => return Err(format!("wait: {error}")),
      }
    }
  })();
  drop(backend);
  if result.is_err() {
    let mut batch =
      Batch { version: VERSION, reply_count: 1, ..Default::default() };
    batch.replies[0] = Message {
      version: VERSION,
      kind: Reply::Failed as u32,
      epoch: current.epoch,
      generation: current.generation,
      ..Default::default()
    };
    let _ = control.exchange(&mut batch);
  }
  let stats = Statistics {
    exchanges,
    registrations,
    events,
    replies,
    max_events,
    max_replies,
  };
  eprintln!(
    "rkmpp session stats: exchanges={} registrations={} events={} \
     replies={} max_events={} max_replies={}",
    stats.exchanges,
    stats.registrations,
    stats.events,
    stats.replies,
    stats.max_events,
    stats.max_replies
  );
  result.map(|()| stats)
}
