//! module: service; responsibility: supervise the external bridge
//! daemon lifetime. operations: serve; entities: none; tests:
//! service_tests and Python process acceptance.
use std::os::fd::AsFd;
use std::path::Path;
use std::time::Duration;

use crate::diagnostics::Endpoint;
use crate::shutdown::Signals;
use crate::transport::{Admission, Broker};
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
  let mut workers = supervisor::Supervisor::new(shutdown.clone())
    .map_err(|e| format!("create supervisor: {e}"))?;
  let exits = workers.notification();
  let result = (|| {
    while !shutdown.stopping() {
      workers.reap();
      let session = match broker.accept(&signals, &exits) {
        Ok(Admission::Accepted(session)) => session,
        Ok(Admission::Idle) => continue,
        Ok(Admission::Rejected(error)) => {
          eprintln!("rkmpp admission rejected: {error}");
          workers.reap();
          continue;
        }
        Ok(Admission::Pressure(error)) => {
          eprintln!("rkmpp admission pressure: {error}");
          workers.reap();
          signals
            .wait(Duration::from_millis(100))
            .map_err(|e| format!("admission wait: {e}"))?;
          continue;
        }
        Err(error) => return Err(format!("accept control: {error}")),
      };
      workers.reap();
      let shutdown = shutdown.clone();
      if let Err(error) = workers.start(move || {
        if let Err(error) = driver::run(session, shutdown) {
          eprintln!("rkmpp session: {error}");
        }
      }) {
        // Failed spawn drops the closure and its owned session. Other
        // workers retain their channels and shutdown state.
        eprintln!("rkmpp worker admission failed: {error}");
        signals
          .wait(Duration::from_millis(100))
          .map_err(|e| format!("admission wait: {e}"))?;
      }
    }
    Ok(())
  })();
  shutdown.request();
  workers.join();
  drop(endpoint);
  result
}
