//! module: service::supervisor; responsibility: retain and join every
//! owned decoder worker. operations: start, reap, join; entities:
//! Supervisor; tests: supervisor_tests.
use std::{io, thread};
#[derive(Default)]
pub(super) struct Supervisor {
  workers: Vec<thread::JoinHandle<()>>,
}
impl Supervisor {
  pub(super) fn start(
    &mut self,
    work: impl FnOnce() + Send + 'static,
  ) -> io::Result<()> {
    self.workers.push(
      thread::Builder::new().name("mpp-session".into()).spawn(work)?,
    );
    Ok(())
  }

  pub(super) fn reap(&mut self) {
    let mut active = Vec::with_capacity(self.workers.len());
    for worker in self.workers.drain(..) {
      if worker.is_finished() {
        join(worker);
      } else {
        active.push(worker);
      }
    }
    self.workers = active;
  }

  pub(super) fn join(self) {
    for worker in self.workers {
      join(worker);
    }
  }
}
fn join(worker: thread::JoinHandle<()>) {
  if let Err(panic) = worker.join() {
    std::panic::resume_unwind(panic);
  }
}
