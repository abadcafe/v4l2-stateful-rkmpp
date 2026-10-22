//! module: service::scheduler; responsibility: schedule one bounded
//! bridge decode session. operations: run, Channel, Engine; entities:
//! dependency contracts and Statistics. tests: scheduler_tests.
//! Dependency traits belong to their scheduling consumer.
use std::collections::VecDeque;
use std::io;

use crate::notify::{Listener, Notify};
use crate::pool::{Direction, Pool};
use crate::protocol::command::{Action, Command, Stop};
use crate::protocol::{
  Batch, Event, MAX_BATCH, Message, Reply, VERSION,
};
use crate::shutdown::Shutdown;
pub(super) trait Channel {
  async fn exchange(&mut self, batch: &mut Batch) -> io::Result<()>;
  async fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<Pool>;
  async fn wait(
    &mut self,
    notification: &Listener,
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
  let mut current = Message::default();
  let mut pools = [0u32; 2];
  let mut fetch = true;
  let mut advance = true;
  let (mut max_events, mut max_replies) = (0u32, 0u32);
  let mut pending = VecDeque::with_capacity(64);
  let (mut exchanges, mut registrations, mut events, mut replies) =
    (0u64, 0u64, 0u64, 0u64);
  let mut batch = Batch { version: VERSION, ..Default::default() };
  let result = (async {
    let notification =
      backend.notification().listen().map_err(|e| e.to_string())?;
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
        if let Err(error) = control.exchange(&mut batch).await {
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
        let direction = match command.action {
          Action::Packet { pool, .. } => {
            Some((0, Direction::Output, pool.get()))
          }
          Action::Capture { pool, .. } => {
            Some((1, Direction::Capture, pool.get()))
          }
          Action::Recycle { pool, .. } => {
            Some((1, Direction::Capture, pool.get()))
          }
          _ => None,
        };
        let mut registration = None;
        if let Some((slot, kind, id)) = direction
          && pools[slot] != id
        {
          let pool =
            match control.register(kind, command.route.epoch, id).await
            {
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
          pools[slot] = id;
        }
        advance = true;
        backend.submit(command, registration).map_err(|e| {
          format!("backend event {}: {e}", message.kind)
        })?;
        if let Action::Reset { scope, .. } = command.action {
          if matches!(scope, Stop::Capture { .. }) {
            pools[1] = 0;
          } else {
            pools = [0; 2];
          }
        }
      }
      let progressed = if advance {
        backend
          .advance()
          .await
          .map_err(|e| format!("backend progress: {e}"))?
      } else {
        false
      };
      advance = progressed;
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
        ready = control.wait(&notification, shutdown) => ready,
        ready = backend.wait() => ready.map(|()| false),
      };
      match ready {
        Ok(control_ready) => {
          fetch = control_ready;
          advance = !control_ready;
        }
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
    let _ = control.exchange(&mut batch).await;
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
