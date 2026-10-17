//! module: task; responsibility: own a thread with observable
//! termination. operations: spawn, finished, join; entities: Task,
//! Exit; tests: task_tests. Exit publishes terminal state before
//! notification, on success and unwind.
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use crate::notify::Notify;
struct Exit {
  finished: Arc<AtomicBool>,
  notify: Notify,
}
impl Drop for Exit {
  fn drop(&mut self) {
    self.finished.store(true, Ordering::Release);
    // The owned Notify keeps its fd alive through thread exit.
    if let Err(error) = self.notify.signal() {
      eprintln!("thread exit notification failed: {error}");
      std::process::abort();
    }
  }
}
pub(crate) struct Task {
  handle: Option<JoinHandle<()>>,
  finished: Arc<AtomicBool>,
}
impl Task {
  pub(crate) fn spawn(
    name: &str,
    notify: Notify,
    work: impl FnOnce() + Send + 'static,
  ) -> io::Result<Self> {
    let finished = Arc::new(AtomicBool::new(false));
    let state = finished.clone();
    let handle =
      thread::Builder::new().name(name.into()).spawn(move || {
        let _exit = Exit { finished: state, notify };
        work();
      })?;
    Ok(Self { handle: Some(handle), finished })
  }

  pub(crate) fn finished(&self) -> bool {
    self.finished.load(Ordering::Acquire)
  }

  pub(crate) fn join(mut self) -> thread::Result<()> {
    self.handle.take().unwrap().join()
  }
}
impl Drop for Task {
  fn drop(&mut self) {
    if let Some(handle) = self.handle.take()
      && let Err(panic) = handle.join()
    {
      if thread::panicking() {
        eprintln!("additional worker panic during owner unwind");
      } else {
        std::panic::resume_unwind(panic);
      }
    }
  }
}
