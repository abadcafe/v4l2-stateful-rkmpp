//! module: execution; responsibility: execute bounded transactions on
//! owned native lanes. operations: Pool start/router/terminated/close,
//! Router lane, Client call/schedule/reclaim; entities: Pool,
//! Router, Client, Pending, Limits, Job, Assignment. tests:
//! execution_tests. State is created and destroyed inside its lane and
//! need not be Send. Cancellation before queue reservation never
//! prepares work; queued transactions retain their receiver separately.
//! Closing rejects new preparation and drains every accepted
//! transaction on its owning lane.
use std::future::{Future, poll_fn};
use std::io;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};

use tokio::sync::{mpsc, oneshot};
mod thread;
use thread::Thread;

type Job<S> = Box<dyn FnOnce(&mut S) + Send>;
struct Assignment(Arc<AtomicUsize>);
impl Drop for Assignment {
  fn drop(&mut self) {
    self.0.fetch_sub(1, Ordering::Release);
  }
}
pub(crate) struct Limits {
  pub workers: NonZeroUsize,
  pub backlog: NonZeroUsize,
}
pub(crate) struct Pending<T>(oneshot::Receiver<io::Result<T>>);
impl<T> Future for Pending<T> {
  type Output = io::Result<T>;

  fn poll(
    mut self: Pin<&mut Self>,
    context: &mut Context<'_>,
  ) -> Poll<Self::Output> {
    Pin::new(&mut self.0)
      .poll(context)
      .map(|result| result.unwrap_or_else(|_| Err(closed())))
  }
}
pub(crate) struct Client<S> {
  sender: mpsc::Sender<Job<S>>,
  dirty: Arc<AtomicBool>,
  stopping: Arc<AtomicBool>,
  occupancy: Arc<AtomicUsize>,
  assignment: Option<Arc<Assignment>>,
}
impl<S> Clone for Client<S> {
  fn clone(&self) -> Self {
    Self {
      sender: self.sender.clone(),
      dirty: self.dirty.clone(),
      stopping: self.stopping.clone(),
      occupancy: self.occupancy.clone(),
      assignment: self.assignment.clone(),
    }
  }
}
pub(crate) struct Router<S> {
  lanes: Arc<[Client<S>]>,
  next: Arc<AtomicUsize>,
}
impl<S> Clone for Router<S> {
  fn clone(&self) -> Self {
    Self { lanes: self.lanes.clone(), next: self.next.clone() }
  }
}
pub(crate) struct Pool<S> {
  router: Router<S>,
  workers: Vec<Thread>,
  stopping: Arc<AtomicBool>,
}
fn closed() -> io::Error {
  io::Error::new(io::ErrorKind::BrokenPipe, "execution lane stopped")
}
impl<S: 'static> Client<S> {
  pub(crate) async fn schedule<
    T: Send + 'static,
    W: FnOnce(&mut S) -> io::Result<T> + Send + 'static,
  >(
    &self,
    prepare: impl FnOnce() -> W,
  ) -> io::Result<Pending<T>> {
    if self.stopping.load(Ordering::Acquire) {
      return Err(closed());
    }
    let permit = self.sender.reserve().await.map_err(|_| closed())?;
    if self.stopping.load(Ordering::Acquire) {
      return Err(closed());
    }
    let work = prepare();
    let (sender, receiver) = oneshot::channel();
    let assignment = self.assignment.clone();
    permit.send(Box::new(move |state| {
      // Cancellation can drop every client before accepted native
      // work finishes. That work still occupies its logical lane.
      let result = work(state);
      drop(assignment);
      let _ = sender.send(result);
    }));
    Ok(Pending(receiver))
  }

  pub(crate) async fn call<T: Send + 'static>(
    &self,
    work: impl FnOnce(&mut S) -> io::Result<T> + Send + 'static,
  ) -> io::Result<T> {
    self.schedule(|| work).await?.await
  }

  pub(crate) fn reclaim(&self) {
    self.dirty.store(true, Ordering::Release);
    let _ = self.sender.try_send(Box::new(|_| {}));
  }
}
impl<S: 'static> Router<S> {
  pub(crate) fn lane(&self) -> Client<S> {
    let start =
      self.next.fetch_add(1, Ordering::Relaxed) % self.lanes.len();
    let selected = (0..self.lanes.len())
      .map(|offset| (start + offset) % self.lanes.len())
      .min_by_key(|&index| {
        self.lanes[index].occupancy.load(Ordering::Acquire)
      })
      .unwrap();
    let mut client = self.lanes[selected].clone();
    client.occupancy.fetch_add(1, Ordering::Relaxed);
    // One logical assignment survives all clients and their accepted
    // work. Live sessions keep their original thread affinity.
    client.assignment =
      Some(Arc::new(Assignment(client.occupancy.clone())));
    client
  }
}
impl<S: 'static> Pool<S> {
  pub(crate) fn start(
    name: &str,
    limits: Limits,
    make: impl Fn() -> S + Send + Sync + 'static,
    collect: fn(&mut S),
  ) -> io::Result<Self> {
    let make = Arc::new(make);
    let stopping = Arc::new(AtomicBool::new(false));
    let mut clients = Vec::new();
    let mut workers = Vec::new();
    for _ in 0..limits.workers.get() {
      let (sender, mut receiver) =
        mpsc::channel::<Job<S>>(limits.backlog.get());
      let dirty = Arc::new(AtomicBool::new(false));
      let abandoned = dirty.clone();
      let stop = stopping.clone();
      let initialize = make.clone();
      let worker = Thread::spawn(name, move || {
        let mut state = initialize();
        while let Some(job) = receiver.blocking_recv() {
          if stop.load(Ordering::Acquire) {
            // Receiver closure rejects new reservations while retaining
            // every already accepted transaction for FIFO execution.
            receiver.close();
          }
          if abandoned.swap(false, Ordering::AcqRel) {
            collect(&mut state);
          }
          job(&mut state);
          if abandoned.swap(false, Ordering::AcqRel) {
            collect(&mut state);
          }
        }
      })?;
      clients.push(Client {
        sender,
        dirty,
        stopping: stopping.clone(),
        occupancy: Arc::new(AtomicUsize::new(0)),
        assignment: None,
      });
      workers.push(worker);
    }
    Ok(Self {
      router: Router {
        lanes: clients.into(),
        next: Arc::new(AtomicUsize::new(0)),
      },
      workers,
      stopping,
    })
  }

  pub(crate) fn router(&self) -> Router<S> {
    self.router.clone()
  }

  pub(crate) async fn terminated(&mut self) -> std::thread::Result<()> {
    poll_fn(|context| {
      for worker in &mut self.workers {
        let mut finished = std::pin::pin!(worker.terminated());
        if let Poll::Ready(result) = finished.as_mut().poll(context) {
          return Poll::Ready(result);
        }
      }
      Poll::Pending
    })
    .await
  }

  pub(crate) async fn close(mut self) -> std::thread::Result<()> {
    self.stop();
    let mut result = Ok(());
    for worker in &mut self.workers {
      if let Err(panic) = worker.terminated().await
        && result.is_ok()
      {
        result = Err(panic);
      }
    }
    result
  }
}
impl<S> Pool<S> {
  fn stop(&self) {
    self.stopping.store(true, Ordering::Release);
    for lane in self.router.lanes.iter() {
      let _ = lane.sender.try_send(Box::new(|_| {}));
    }
  }
}
impl<S> Drop for Pool<S> {
  fn drop(&mut self) {
    self.stop();
  }
}
