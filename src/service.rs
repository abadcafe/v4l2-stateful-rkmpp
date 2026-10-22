//! module: service; responsibility: supervise the asynchronous bridge
//! lifetime. operations: serve; entities: none; tests: service_tests
//! and process acceptance.
use std::os::fd::AsFd;
use std::path::Path;
use std::time::Duration;

use crate::diagnostics::Endpoint;
use crate::hardware::Hardware;
use crate::shutdown::Signals;
use crate::transport::{Admission, Broker};
mod scheduler;
#[cfg(test)]
mod scheduler_tests;
const MAX_SESSIONS: usize = 64;
pub async fn serve(
  path: &Path,
  diagnostics: &Path,
) -> Result<(), String> {
  tokio::task::LocalSet::new().run_until(run(path, diagnostics)).await
}
async fn run(path: &Path, diagnostics: &Path) -> Result<(), String> {
  let signals = Signals::install()
    .map_err(|e| format!("install termination signals: {e}"))?;
  let shutdown = signals.shutdown();
  let mut broker = Broker::open(path)
    .map_err(|e| format!("open control {}: {e}", path.display()))?;
  let control =
    broker.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
  let mut endpoint =
    Endpoint::open(diagnostics, control.as_fd(), &shutdown)
      .await
      .map_err(|e| {
        format!("open diagnostics {}: {e}", diagnostics.display())
      })?;
  let (mut hardware, client) =
    Hardware::start().map_err(|e| e.to_string())?;
  let mut sessions = tokio::task::JoinSet::new();
  eprintln!("rkmpp service listening on {}", path.display());
  let mut panic = None;
  let result = loop {
    tokio::select! {
      result = hardware.terminated() => {
        if let Err(payload) = result { panic = Some(payload); }
        break Err("hardware executor terminated".into());
      },
      result = endpoint.terminated() => {
        if let Err(error) = result && error.is_panic() { panic = Some(error.into_panic()); }
        break Err("diagnostics task terminated".into());
      },
      result = sessions.join_next(), if !sessions.is_empty() => {
        if let Some(Err(error)) = result {
          let message = format!("session task failed: {error}");
          if error.is_panic() { panic = Some(error.into_panic()); }
          break Err(message);
        }
      },
      result = signals.wait(Duration::from_secs(86400)) => {
        if let Err(error) = result { break Err(format!("termination wait: {error}")); }
        if shutdown.stopping() { break Ok(()); }
      },
      // Lane queues bound submitted work, not the tasks and descriptor
      // owners waiting to submit it. Reserve capacity for the entire
      // session lifetime before consuming another broker offer.
      admission = broker.accept(), if sessions.len() < MAX_SESSIONS => {
        match admission {
          Ok(Admission::Accepted(session)) => {
            let stop = shutdown.clone();
            let client = client.clone();
            sessions.spawn_local(async move {
              if let Err(error) = run_session(session, stop, client).await {
                eprintln!("rkmpp session: {error}");
              }
            });
          },
          Ok(Admission::Idle) => {},
          Ok(Admission::Rejected(error)) => eprintln!("rkmpp admission rejected: {error}"),
          Ok(Admission::Pressure(error)) => {
            eprintln!("rkmpp admission pressure: {error}");
            if let Err(error) = signals.wait(Duration::from_millis(100)).await {
              break Err(format!("admission wait: {error}"));
            }
            if shutdown.stopping() { break Ok(()); }
          },
          Err(error) => break Err(format!("accept control: {error}")),
        }
      },
    }
  };
  shutdown.request();
  let mut result = result;
  while let Some(joined) = sessions.join_next().await {
    if let Err(error) = joined {
      result = Err(format!("session task failed: {error}"));
      if error.is_panic() && panic.is_none() {
        panic = Some(error.into_panic());
      }
    }
  }
  if let Err(error) = hardware.close().await {
    result = Err("hardware executor panicked".into());
    if panic.is_none() {
      panic = Some(error);
    }
  }
  if let Err(error) = endpoint.close().await {
    result = Err(format!("diagnostics task failed: {error}"));
    if error.is_panic() && panic.is_none() {
      panic = Some(error.into_panic());
    }
  }
  drop(control);
  if let Err(payload) = broker.close().await {
    result = Err("kernel transport executor panicked".into());
    if panic.is_none() {
      panic = Some(payload);
    }
  }
  if let Some(panic) = panic {
    std::panic::resume_unwind(panic);
  }
  result
}

use std::io;

use scheduler::{Channel, Engine};

use crate::hardware::{Client, Engine as HardwareEngine};
use crate::notify::{Listener, Notify};
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{Batch, Message};
use crate::shutdown::Shutdown;
use crate::transport::Session;

impl Channel for Session {
  async fn exchange(&mut self, batch: &mut Batch) -> io::Result<()> {
    Session::exchange(self, batch).await
  }

  async fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<Pool> {
    Session::register(self, direction, epoch, id).await
  }

  async fn wait(
    &mut self,
    notification: &Listener,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    Session::wait(self, notification, shutdown).await
  }
}
impl Engine for HardwareEngine {
  async fn close(self) -> io::Result<()> {
    HardwareEngine::close(self).await
  }

  fn notification(&self) -> Notify {
    HardwareEngine::notification(self)
  }

  fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()> {
    HardwareEngine::submit(self, command, pool)
  }

  async fn advance(&mut self) -> io::Result<bool> {
    HardwareEngine::advance(self).await
  }

  async fn wait(&self) -> io::Result<()> {
    HardwareEngine::wait(self).await
  }

  fn take_reply(&mut self) -> Option<Message> {
    HardwareEngine::take_reply(self)
  }
}
pub(crate) async fn run_session(
  mut session: Session,
  shutdown: Shutdown,
  client: Client,
) -> Result<(), String> {
  let backend =
    client.open(session.lease()).await.map_err(|e| e.to_string())?;
  scheduler::run(&mut session, backend, &shutdown).await.map(|_| ())
}
