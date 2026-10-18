//! module: service::scheduler; responsibility: schedule one bounded
//! bridge decode session. operations: run, Channel, Engine; entities:
//! dependency contracts and Statistics. tests: scheduler_tests.
//! Dependency traits belong to their scheduling consumer.
use std::collections::VecDeque;
use std::io;
use std::os::fd::AsFd;

use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{
  Batch, Event, MAX_BATCH, Message, Reply, VERSION,
};
use crate::readiness::Readable;
use crate::shutdown::Shutdown;
pub(super) trait Channel {
  fn exchange(&mut self, batch: &mut Batch) -> io::Result<()>;
  fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<Pool>;
  async fn wait(
    &mut self,
    notify: &Notify,
    notification: &Readable,
    shutdown: &Shutdown,
  ) -> io::Result<bool>;
}
pub(super) trait Engine: Sized {
  async fn close(self) -> io::Result<()>;
  fn notification(&self) -> Notify;
  fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()>;
  async fn advance(&mut self) -> io::Result<bool>;
  async fn wait(&self) -> io::Result<()>;
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
pub(super) async fn run(
  control: &mut impl Channel,
  mut backend: impl Engine,
  shutdown: &Shutdown,
) -> Result<Statistics, String> {
  let notify = backend.notification();
  let notification = Readable::new(
    notify.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?,
  )
  .map_err(|e| e.to_string())?;
  let mut current = Message::default();
  let mut pools = [0u32; 2];
  let mut fetch = true;
  let (mut max_events, mut max_replies) = (0u32, 0u32);
  let mut pending = VecDeque::with_capacity(64);
  let (mut exchanges, mut registrations, mut events, mut replies) =
    (0u64, 0u64, 0u64, 0u64);
  let mut batch = Batch { version: VERSION, ..Default::default() };
  let result = (async {
    loop {
      if shutdown.stopping() {
        return Ok(());
      }
      batch.event_count = 0;
      batch.reply_count = 0;
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
        let mut registration = None;
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
          registration = Some(pool);
          registrations += 1;
          pools[slot] = message.pool_id;
        }
        backend.submit(command, registration).map_err(|e| {
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
      let progressed = backend
        .advance()
        .await
        .map_err(|e| format!("backend progress: {e}"))?;
      while let Some(reply) = backend.take_reply() {
        pending.push_back(reply);
      }
      if pending.len() > 64 {
        return Err("reply backpressure overflow".into());
      }
      if !pending.is_empty() || fetch || progressed {
        if progressed {
          fetch = true;
        }
        tokio::task::yield_now().await;
        continue;
      }
      let ready = tokio::select! {
        ready = control.wait(&notify, &notification, shutdown) => ready,
        ready = backend.wait() => ready.map(|()| false),
      };
      match ready {
        Ok(control_ready) => fetch = control_ready,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
        Err(error) => return Err(format!("wait: {error}")),
      }
    }
  })
  .await;
  let closed =
    backend.close().await.map_err(|e| format!("close backend: {e}"));
  let result = result.and(closed);
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
