//! module: execution::thread; responsibility: join native execution
//! without blocking the asynchronous supervisor while work is active.
//! operations: spawn, terminated; entities: Thread, Exit; tests:
//! execution_tests.
use std::io;

use tokio::sync::oneshot;

struct Exit(Option<oneshot::Sender<()>>);
impl Drop for Exit {
  fn drop(&mut self) {
    if let Some(sender) = self.0.take() {
      let _ = sender.send(());
    }
  }
}
pub(super) struct Thread {
  handle: Option<std::thread::JoinHandle<()>>,
  finished: oneshot::Receiver<()>,
}
impl Thread {
  pub(super) fn spawn(
    name: &str,
    work: impl FnOnce() + Send + 'static,
  ) -> io::Result<Self> {
    let (sender, finished) = oneshot::channel();
    let handle = std::thread::Builder::new()
      .name(name.to_owned())
      .spawn(move || {
        // This guard is destroyed after the work's owned state,
        // including during unwinding. Joining preserves the
        // original panic payload.
        let _exit = Exit(Some(sender));
        work();
      })
      .map_err(|error| {
        io::Error::new(
          error.kind(),
          format!("spawn worker thread: {error}"),
        )
      })?;
    Ok(Self { handle: Some(handle), finished })
  }

  pub(super) async fn terminated(&mut self) -> std::thread::Result<()> {
    if self.handle.is_none() {
      return Ok(());
    }
    let _ = (&mut self.finished).await;
    self.handle.take().unwrap().join()
  }
}
