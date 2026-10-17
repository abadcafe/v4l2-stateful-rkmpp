//! module: service::supervisor; responsibility: own session worker
//! lifetimes. operations: new, notification, start, reap, join;
//! entities: Supervisor; tests: supervisor_tests. Every exit wakes
//! admission; teardown stops and joins all workers before propagating
//! the first programming panic.
use std::io;

use crate::notify::Notify;
use crate::shutdown::Shutdown;
use crate::task::Task;
pub(super) struct Supervisor {
  workers: Vec<Task>,
  notify: Notify,
  shutdown: Shutdown,
}
impl Supervisor {
  pub(super) fn new(shutdown: Shutdown) -> io::Result<Self> {
    Ok(Self { workers: Vec::new(), notify: Notify::new()?, shutdown })
  }

  pub(super) fn notification(&self) -> Notify {
    self.notify.clone()
  }

  pub(super) fn start(
    &mut self,
    work: impl FnOnce() + Send + 'static,
  ) -> io::Result<()> {
    self.workers.push(Task::spawn(
      "mpp-session",
      self.notify.clone(),
      work,
    )?);
    Ok(())
  }

  pub(super) fn reap(&mut self) {
    let mut index = 0;
    while index < self.workers.len() {
      if self.workers[index].finished() {
        if let Err(panic) = self.workers.swap_remove(index).join() {
          std::panic::resume_unwind(panic);
        }
      } else {
        index += 1;
      }
    }
  }

  pub(super) fn join(mut self) {
    self.finish();
  }

  fn finish(&mut self) {
    self.shutdown.request();
    let mut failure = None;
    for worker in self.workers.drain(..) {
      if let Err(panic) = worker.join() {
        if failure.is_none() {
          failure = Some(panic);
        } else {
          eprintln!("additional session worker panic during teardown");
        }
      }
    }
    if let Some(panic) = failure {
      if std::thread::panicking() {
        eprintln!("session worker panic during owner unwind");
      } else {
        std::panic::resume_unwind(panic);
      }
    }
  }
}
impl Drop for Supervisor {
  fn drop(&mut self) {
    self.finish();
  }
}
