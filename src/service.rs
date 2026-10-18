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
mod driver;
#[cfg(test)]
mod driver_tests;
mod scheduler;
#[cfg(test)]
mod scheduler_tests;
pub async fn serve(
  path: &Path,
  diagnostics: &Path,
) -> Result<(), String> {
  tokio::task::LocalSet::new().run_until(run(path, diagnostics)).await
}
async fn run(path: &Path, diagnostics: &Path) -> Result<(), String> {
  let broker = Broker::open(path)
    .map_err(|e| format!("open control {}: {e}", path.display()))?;
  let signals = Signals::install()
    .map_err(|e| format!("install termination signals: {e}"))?;
  let shutdown = signals.shutdown();
  let mut endpoint =
    Endpoint::open(diagnostics, broker.as_fd(), &shutdown)
      .await
      .map_err(|e| {
        format!("open diagnostics {}: {e}", diagnostics.display())
      })?;
  let mut hardware = Hardware::start();
  let mut sessions = tokio::task::JoinSet::new();
  eprintln!("rkmpp service listening on {}", path.display());
  let mut panic = None;
  let result = loop {
    tokio::select! {
      result = hardware.terminated() => {
        if let Err(error) = result && error.is_panic() { panic = Some(error.into_panic()); }
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
      admission = broker.accept() => {
        match admission {
          Ok(Admission::Accepted(session)) => {
            let stop = shutdown.clone();
            let client = hardware.client();
            sessions.spawn_local(async move {
              if let Err(error) = driver::run(session, stop, client).await {
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
    result = Err(format!("hardware executor failed: {error}"));
    if error.is_panic() && panic.is_none() {
      panic = Some(error.into_panic());
    }
  }
  if let Err(error) = endpoint.close().await {
    result = Err(format!("diagnostics task failed: {error}"));
    if error.is_panic() && panic.is_none() {
      panic = Some(error.into_panic());
    }
  }
  if let Some(panic) = panic {
    std::panic::resume_unwind(panic);
  }
  result
}
