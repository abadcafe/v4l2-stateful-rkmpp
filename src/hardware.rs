//! module: hardware; responsibility: confine synchronous decoder and
//! GPU work to one owned blocking executor. operations:
//! start/client/open/submit/advance/wait/close/terminated;
//! entities: Hardware, Client, Engine, Store, Entry, Owner, Job. tests:
//! hardware_tests and service process acceptance. Requests are bounded;
//! abandoned engines are reclaimed at every dispatch boundary,
//! including idle. Each engine retains its in-flight turn across
//! cancellation; a later advance receives that turn exactly once.
use std::collections::BTreeMap;
mod dispatch;
#[cfg(test)]
mod dispatch_tests;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use dispatch::Dispatch;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::backend::Backend;
use crate::notify::Notify;
use crate::pool::Pool;
use crate::protocol::Message;
use crate::protocol::command::Command;
struct Entry {
  owner: Weak<()>,
  backend: Backend,
  _lease: Box<dyn Send>,
}
struct Store {
  entries: BTreeMap<u64, Entry>,
  next: u64,
  stop: bool,
}
type Job = Box<dyn FnOnce(&mut Store) + Send>;
#[derive(Clone)]
pub struct Client(mpsc::Sender<Job>, Arc<AtomicBool>);
pub struct Hardware {
  client: Client,
  task: Option<JoinHandle<()>>,
  stop: Arc<AtomicBool>,
}
pub struct Engine {
  client: Client,
  id: u64,
  owner: Option<Owner>,
  notify: Notify,
  dispatch: Dispatch,
  in_flight: Option<oneshot::Receiver<io::Result<dispatch::Completed>>>,
  completion: Option<crate::readiness::Readable>,
}
struct Owner {
  life: Option<Arc<()>>,
  client: Client,
}
impl Drop for Owner {
  fn drop(&mut self) {
    self.life = None;
    self.client.1.store(true, Ordering::Release);
    // Covers cancellation before open returns as well as a dropped
    // engine. A full queue already guarantees a subsequent
    // collection boundary.
    let _ = self.client.0.try_send(Box::new(|_| {}));
  }
}
fn closed() -> io::Error {
  io::Error::new(io::ErrorKind::BrokenPipe, "hardware executor stopped")
}
impl Client {
  async fn call<T: Send + 'static>(
    &self,
    work: impl FnOnce(&mut Store) -> io::Result<T> + Send + 'static,
  ) -> io::Result<T> {
    let (tx, rx) = oneshot::channel();
    self
      .0
      .send(Box::new(move |store| {
        let _ = tx.send(work(store));
      }))
      .await
      .map_err(|_| closed())?;
    rx.await.map_err(|_| closed())?
  }

  pub async fn open(
    &self,
    lease: impl Send + 'static,
  ) -> io::Result<Engine> {
    let life = Arc::new(());
    let weak = Arc::downgrade(&life);
    let owner = Owner { life: Some(life), client: self.clone() };
    let (id, notify) = self
      .call(move |store| {
        let backend = Backend::new()?;
        let notify = backend.notification();
        let id = store.next;
        store.next = store.next.checked_add(1).ok_or_else(|| {
          io::Error::other("hardware session identity exhausted")
        })?;
        if weak.strong_count() == 0 {
          return Err(closed());
        }
        store.entries.insert(
          id,
          Entry { owner: weak, backend, _lease: Box::new(lease) },
        );
        Ok((id, notify))
      })
      .await?;
    Ok(Engine {
      client: self.clone(),
      id,
      owner: Some(owner),
      notify,
      dispatch: Dispatch::default(),
      in_flight: None,
      completion: None,
    })
  }
}
impl Hardware {
  pub fn start() -> Self {
    let (tx, mut rx) = mpsc::channel::<Job>(64);
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let dirty = Arc::new(AtomicBool::new(false));
    let abandoned = dirty.clone();
    let task = tokio::task::spawn_blocking(move || {
      let mut store =
        Store { entries: BTreeMap::new(), next: 1, stop: false };
      while let Some(job) = rx.blocking_recv() {
        if abandoned.swap(false, Ordering::AcqRel) {
          store
            .entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        }
        job(&mut store);
        if abandoned.swap(false, Ordering::AcqRel) {
          store
            .entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        }
        if store.stop || stopping.load(Ordering::Acquire) {
          break;
        }
      }
      // All MPP/Vulkan contexts are destroyed on their execution
      // thread.
    });
    Self { client: Client(tx, dirty), task: Some(task), stop }
  }

  pub fn client(&self) -> Client {
    self.client.clone()
  }

  pub async fn terminated(
    &mut self,
  ) -> Result<(), tokio::task::JoinError> {
    let result = self.task.as_mut().unwrap().await;
    self.task = None;
    result
  }

  pub async fn close(mut self) -> Result<(), tokio::task::JoinError> {
    if self.task.is_some() {
      let _ = self
        .client
        .call(|store| {
          store.stop = true;
          Ok(())
        })
        .await;
      self.terminated().await
    } else {
      Ok(())
    }
  }
}
impl Engine {
  pub fn notification(&self) -> Notify {
    self.notify.clone()
  }

  pub fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()> {
    if self.in_flight.is_some() {
      return Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "receive the in-flight hardware turn before submitting work",
      ));
    }
    self.dispatch.submit(command, pool)
  }

  pub async fn advance(&mut self) -> io::Result<bool> {
    if self.in_flight.is_none() {
      // Cancellation while waiting for capacity must leave the queued
      // batch intact. After reservation, sending and retaining the
      // receiver contain no suspension point.
      let permit =
        self.client.0.reserve().await.map_err(|_| closed())?;
      let mut dispatch = std::mem::take(&mut self.dispatch);
      let id = self.id;
      let (sender, receiver) = oneshot::channel();
      permit.send(Box::new(move |store| {
        let result = (|| {
          let backend =
            &mut store.entries.get_mut(&id).ok_or_else(closed)?.backend;
          let runnable = dispatch.advance(backend)?;
          Ok(dispatch::Completed {
            runnable,
            dispatch,
            completion: backend.completion(),
          })
        })();
        let _ = sender.send(result);
      }));
      self.in_flight = Some(receiver);
    }
    let received = self.in_flight.as_mut().unwrap().await;
    self.in_flight = None;
    let dispatch::Completed { runnable, dispatch, completion } =
      received.map_err(|_| closed())??;
    self.dispatch = dispatch;
    use std::os::fd::{AsFd, AsRawFd};
    if self.completion.as_ref().map(|r| r.as_fd().as_raw_fd())
      != completion.as_ref().map(AsRawFd::as_raw_fd)
    {
      self.completion = completion
        .map(crate::readiness::Readable::from_lease)
        .transpose()?;
    }
    Ok(runnable)
  }

  pub async fn wait(&self) -> io::Result<()> {
    if let Some(ready) = &self.completion {
      tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ready.wait(),
      )
      .await
      .map_err(|_| {
        io::Error::new(
          io::ErrorKind::TimedOut,
          "GPU completion timed out",
        )
      })?
    } else {
      std::future::pending().await
    }
  }

  pub fn take_reply(&mut self) -> Option<Message> {
    self.dispatch.take_reply()
  }

  pub async fn close(mut self) -> io::Result<()> {
    self.owner = None;
    // The executor collects dead owners before acknowledging this
    // barrier.
    self.client.call(|_| Ok(())).await
  }
}
impl Drop for Hardware {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    let _ = self.client.0.try_send(Box::new(|_| {}));
  }
}
