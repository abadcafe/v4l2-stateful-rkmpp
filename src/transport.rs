//! module: transport; responsibility: transfer owned bridge resources
//! without blocking the runtime. operations: Broker open/accept/close,
//! Session new/exchange/register/wait; entities: Broker, Session,
//! Flight, Registration. tests: transport_tests, kernel and service
//! process acceptance. All lanes are created at startup; a session
//! retains each in-flight ioctl across cancellation.
use std::fs::OpenOptions;
use std::io;
use std::num::NonZeroUsize;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::descriptor::Lease;
use crate::execution::{self, Limits, Pending, Pool as Executors};
use crate::notify::Notify;
use crate::pool::{Direction, Pool as RegisteredPool};
use crate::protocol::{Batch, MAX_BATCH, Message};
use crate::readiness::Readable;
use crate::shutdown::Shutdown;
mod admission;
#[cfg(test)]
mod admission_tests;
mod ioctl;
mod registration;
#[cfg(test)]
mod registration_tests;
pub use admission::Admission;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Registration {
  direction: Direction,
  epoch: u64,
  id: u32,
}
enum Flight {
  Exchange { result: Pending<Batch> },
  Register { key: Registration, result: Pending<RegisteredPool> },
}
pub struct Session {
  ready: Readable,
  client: execution::Client<()>,
  flight: Option<Flight>,
  // The request snapshot survives cancellation. Reuse its storage for
  // the whole session instead of allocating once per ioctl exchange.
  submitted: Vec<Message>,
}
pub struct Broker {
  ready: Readable,
  acceptors: Executors<()>,
  channels: Executors<()>,
  acceptor: execution::Client<()>,
  accepting: Option<Pending<Admission<OwnedFd>>>,
  panic: Option<Box<dyn std::any::Any + Send>>,
}
fn limits(workers: usize) -> Limits {
  Limits {
    workers: NonZeroUsize::new(workers).unwrap(),
    backlog: NonZeroUsize::new(64).unwrap(),
  }
}
impl Session {
  pub fn new(
    fd: OwnedFd,
    client: execution::Client<()>,
  ) -> io::Result<Self> {
    Ok(Self {
      ready: Readable::new(fd)?,
      client,
      flight: None,
      submitted: Vec::with_capacity(MAX_BATCH),
    })
  }

  pub fn lease(&self) -> Lease {
    self.ready.lease()
  }

  pub async fn exchange(
    &mut self,
    batch: &mut Batch,
  ) -> io::Result<()> {
    crate::protocol::validate(batch)?;
    if self.flight.is_none() {
      let lease = self.lease();
      let result = self
        .client
        .schedule(|| {
          self.submitted.clear();
          self.submitted.extend_from_slice(
            &batch.replies[..batch.reply_count as usize],
          );
          let owned = *batch;
          move |_| ioctl::exchange(lease, owned)
        })
        .await?;
      self.flight = Some(Flight::Exchange { result });
    }
    let Some(Flight::Exchange { result }) = self.flight.as_mut() else {
      return Err(busy());
    };
    if self.submitted.as_slice()
      != &batch.replies[..batch.reply_count as usize]
    {
      return Err(busy());
    }
    let completed = result.await;
    self.flight = None;
    *batch = completed?;
    Ok(())
  }

  pub async fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<RegisteredPool> {
    let key = Registration { direction, epoch, id };
    if self.flight.is_none() {
      let lease = self.lease();
      let result = self
        .client
        .schedule(|| {
          move |_| ioctl::register(lease, direction, epoch, id)
        })
        .await?;
      self.flight = Some(Flight::Register { key, result });
    }
    let Some(Flight::Register { key: active, result }) =
      self.flight.as_mut()
    else {
      return Err(busy());
    };
    if *active != key {
      return Err(busy());
    }
    let completed = result.await;
    self.flight = None;
    completed
  }

  pub async fn wait(
    &self,
    notify: &Notify,
    notification: &Readable,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    tokio::select! {
      result = self.ready.wait() => { result?; Ok(true) },
      result = notification.wait() => { result?; notify.drain()?; Ok(false) },
      () = shutdown.cancelled() => Ok(false),
    }
  }
}
fn busy() -> io::Error {
  io::Error::new(
    io::ErrorKind::WouldBlock,
    "another ioctl transaction is in flight",
  )
}
impl AsFd for Broker {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.ready.as_fd()
  }
}
impl Broker {
  pub fn open(path: &Path) -> io::Result<Self> {
    let fd: OwnedFd = OpenOptions::new()
      .read(true)
      .write(true)
      .custom_flags(libc::O_NONBLOCK)
      .open(path)?
      .into();
    let ready = Readable::new(fd)?;
    let acceptors =
      Executors::start("rkmpp-accept", limits(1), || (), |_| {})?;
    let acceptor = acceptors.router().lane();
    let channels =
      Executors::start("rkmpp-channel", limits(4), || (), |_| {})?;
    Ok(Self {
      ready,
      acceptors,
      channels,
      acceptor,
      accepting: None,
      panic: None,
    })
  }

  fn failed(&mut self, result: std::thread::Result<()>) -> io::Error {
    if let Err(payload) = result {
      self.panic = Some(payload);
    }
    io::Error::other("kernel transport executor terminated")
  }

  pub async fn accept(&mut self) -> io::Result<Admission<Session>> {
    if self.accepting.is_none() {
      tokio::select! {
        result=self.acceptors.terminated()=>return Err(self.failed(result)),
        result=self.channels.terminated()=>return Err(self.failed(result)),
        result=self.ready.wait()=>result?,
      }
      let lease = self.ready.lease();
      self.accepting = Some(
        self
          .acceptor
          .schedule(|| move |_| ioctl::accept(lease))
          .await?,
      );
    }
    let result = tokio::select! {
      result=self.acceptors.terminated()=>return Err(self.failed(result)),
      result=self.channels.terminated()=>return Err(self.failed(result)),
      result=self.accepting.as_mut().unwrap()=>result,
    };
    self.accepting = None;
    Ok(match result? {
      Admission::Accepted(fd) => Admission::Accepted(Session::new(
        fd,
        self.channels.router().lane(),
      )?),
      Admission::Idle => Admission::Idle,
      Admission::Rejected(error) => Admission::Rejected(error),
      Admission::Pressure(error) => Admission::Pressure(error),
    })
  }

  pub async fn close(self) -> std::thread::Result<()> {
    let first = self.acceptors.close().await;
    let second = self.channels.close().await;
    match self.panic {
      Some(payload) => Err(payload),
      None => first.and(second),
    }
  }
}
