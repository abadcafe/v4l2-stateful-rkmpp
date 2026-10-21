//! module: hardware; responsibility: own decoder sessions on bounded
//! native execution lanes. operations:
//! start/open/advance/close; entities: Hardware, Client,
//! Engine, Store, Entry, Owner; tests: hardware_tests and service
//! process acceptance. Each session has fixed thread affinity;
//! cancellation retains its queued turn and resource leases.
use std::collections::BTreeMap;
use std::io;
use std::num::NonZeroUsize;
use std::sync::{Arc, Weak};

use crate::backend::Backend;
use crate::execution::{self, Limits, Pending, Pool as Executors};
use crate::notify::Notify;
use crate::pool::Pool;
use crate::protocol::Message;
use crate::protocol::command::Command;
mod dispatch;
#[cfg(test)]
mod dispatch_tests;
use dispatch::Dispatch;
struct Entry {
  owner: Weak<()>,
  backend: Backend,
  _lease: Box<dyn Send>,
}
struct Store {
  factory: crate::quantize::Factory,
  entries: BTreeMap<u64, Entry>,
  next: u64,
}
#[derive(Clone)]
pub struct Client(execution::Router<Store>);
pub struct Hardware(Executors<Store>);
pub struct Engine {
  client: execution::Client<Store>,
  id: u64,
  owner: Option<Owner>,
  notify: Notify,
  dispatch: Dispatch,
  in_flight: Option<Pending<dispatch::Completed>>,
  completion: Option<crate::readiness::Readable>,
}
struct Owner {
  life: Option<Arc<()>>,
  client: execution::Client<Store>,
}
impl Drop for Owner {
  fn drop(&mut self) {
    self.life = None;
    self.client.reclaim();
  }
}
fn closed() -> io::Error {
  io::Error::new(io::ErrorKind::BrokenPipe, "hardware executor stopped")
}
impl Client {
  pub async fn open(
    &self,
    lease: impl Send + 'static,
  ) -> io::Result<Engine> {
    let client = self.0.lane();
    let life = Arc::new(());
    let weak = Arc::downgrade(&life);
    let owner = Owner { life: Some(life), client: client.clone() };
    let (id, notify) = client
      .call(move |store| {
        if weak.strong_count() == 0 {
          return Err(closed());
        }
        let backend = Backend::new(store.factory.clone())?;
        let notify = backend.notification();
        let id = store.next;
        store.next = store.next.checked_add(1).ok_or_else(|| {
          io::Error::other("hardware session identity exhausted")
        })?;
        store.entries.insert(
          id,
          Entry { owner: weak, backend, _lease: Box::new(lease) },
        );
        Ok((id, notify))
      })
      .await?;
    Ok(Engine {
      client,
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
  pub fn start() -> io::Result<(Self, Client)> {
    let factory = crate::quantize::Factory::default();
    let executor = Executors::start(
      "rkmpp-hardware",
      Limits {
        workers: NonZeroUsize::new(4).unwrap(),
        backlog: NonZeroUsize::new(64).unwrap(),
      },
      move || Store {
        factory: factory.clone(),
        entries: BTreeMap::new(),
        next: 1,
      },
      |store| {
        store.entries.retain(|_, entry| entry.owner.strong_count() != 0)
      },
    )?;
    let client = Client(executor.router());
    Ok((Self(executor), client))
  }

  pub async fn terminated(&mut self) -> std::thread::Result<()> {
    self.0.terminated().await
  }

  pub async fn close(self) -> std::thread::Result<()> {
    self.0.close().await
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
      let id = self.id;
      self.in_flight = Some(
        self
          .client
          .schedule(|| {
            let mut dispatch = std::mem::take(&mut self.dispatch);
            move |store| {
              let backend = &mut store
                .entries
                .get_mut(&id)
                .ok_or_else(closed)?
                .backend;
              let runnable = dispatch.advance(backend)?;
              Ok(dispatch::Completed {
                runnable,
                dispatch,
                completion: backend.completion(),
              })
            }
          })
          .await?,
      );
    }
    let received = self.in_flight.as_mut().unwrap().await;
    self.in_flight = None;
    let dispatch::Completed { runnable, dispatch, completion } =
      received?;
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
