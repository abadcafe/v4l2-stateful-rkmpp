//! module: service; responsibility: supervise the external bridge
//! daemon lifetime. operations: serve; entities: none; tests:
//! service_tests and Python process acceptance.
use std::io;
use std::os::fd::AsFd;
use std::path::Path;

use crate::diagnostics::Endpoint;
use crate::shutdown::Signals;
use crate::transport::Broker;
mod driver;
#[cfg(test)]
mod driver_tests;
mod supervisor;
#[cfg(test)]
mod supervisor_tests;
mod worker;
#[cfg(test)]
mod worker_tests;
pub fn serve(path: &Path, diagnostics: &Path) -> Result<(), String> {
  let broker = Broker::open(path)
    .map_err(|e| format!("open control {}: {e}", path.display()))?;
  let signals = Signals::install()
    .map_err(|e| format!("install termination signals: {e}"))?;
  let shutdown = signals.shutdown();
  let endpoint = Endpoint::open(diagnostics, broker.as_fd(), &shutdown)
    .map_err(|error| {
      format!("open diagnostics {}: {error}", diagnostics.display())
    })?;
  eprintln!("rkmpp service listening on {}", path.display());
  let mut workers = supervisor::Supervisor::default();
  let result = (|| {
    while !shutdown.stopping() {
      let session = match broker.accept(&signals) {
        Ok(Some(session)) => session,
        Ok(None) => continue,
        Err(error)
          if matches!(
            error.kind(),
            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
          ) =>
        {
          continue;
        }
        Err(error) => return Err(format!("accept control: {error}")),
      };
      workers.reap();
      let shutdown = shutdown.clone();
      workers
        .start(move || {
          if let Err(error) = driver::run(session, shutdown) {
            eprintln!("rkmpp session: {error}");
          }
        })
        .map_err(|e| format!("start worker: {e}"))?;
    }
    Ok(())
  })();
  shutdown.request();
  workers.join();
  drop(endpoint);
  result
}
