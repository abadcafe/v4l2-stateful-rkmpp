//! module: converter; responsibility: schedule bounded, owned
//! asynchronous GPU conversions. operations: new, submit, take;
//! entities: Converter, Job, Completion, SubmitError, Input.
//! tests: converter_tests; ownership is returned on backpressure or
//! completion, and Drop joins GPU work.
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex};

use crate::image::{Depth, Image, Plane};
use crate::notify::Notify;
use crate::quantize::Quantizer;
use crate::task::Task;
pub trait Input: Send + 'static {
  fn plane(&self) -> Plane<'_>;
  fn depth(&self) -> Depth {
    Depth::Ten
  }
}
impl Input for Image {
  fn plane(&self) -> Plane<'_> {
    self.plane()
  }
}
pub struct Job<T> {
  token: u32,
  extent: (u32, u32),
  input: T,
  output: Image,
}
impl<T> Job<T> {
  pub fn new(
    token: u32,
    extent: (u32, u32),
    input: T,
    output: Image,
  ) -> Self {
    Self { token, extent, input, output }
  }

  pub fn into_input(self) -> T {
    self.input
  }
}
pub struct Completion<T> {
  job: Job<T>,
  result: io::Result<()>,
}
impl<T> Completion<T> {
  pub fn into_parts(self) -> (u32, T, io::Result<()>) {
    (self.job.token, self.job.input, self.result)
  }
}
pub struct SubmitError<T> {
  job: Job<T>,
  full: bool,
}
impl<T> SubmitError<T> {
  pub fn is_full(&self) -> bool {
    self.full
  }

  pub fn into_job(self) -> Job<T> {
    self.job
  }
}
impl<T> std::fmt::Debug for SubmitError<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("SubmitError").field("full", &self.full).finish()
  }
}
struct State<T> {
  jobs: VecDeque<Job<T>>,
  completed: VecDeque<Completion<T>>,
  outstanding: usize,
  stop: bool,
}
pub struct Converter<T: Input> {
  state: Arc<(Mutex<State<T>>, Condvar)>,
  worker: Option<Task>,
}
impl<T: Input> Converter<T> {
  pub fn new(notify: Notify) -> io::Result<Self> {
    let mut quantizer = Quantizer::new()?;
    let state = Arc::new((
      Mutex::new(State::<T> {
        jobs: VecDeque::with_capacity(3),
        completed: VecDeque::with_capacity(3),
        outstanding: 0,
        stop: false,
      }),
      Condvar::new(),
    ));
    let shared = state.clone();
    let worker =
      Task::spawn("nv15-quantize", notify.clone(), move || {
        let (mutex, ready) = &*shared;
        loop {
          let job = {
            let mut state = mutex.lock().unwrap();
            while !state.stop && state.jobs.is_empty() {
              state = ready.wait(state).unwrap();
            }
            if state.stop {
              break;
            }
            state.jobs.pop_front().unwrap()
          };
          let result = quantizer.convert(
            job.input.depth(),
            job.extent.0,
            job.extent.1,
            job.input.plane(),
            job.output.plane(),
          );
          let mut state = mutex.lock().unwrap();
          state.completed.push_back(Completion { job, result });
          if notify.signal().is_err() {
            state.stop = true;
          }
        }
      })?;
    Ok(Self { state, worker: Some(worker) })
  }

  pub fn submit(&mut self, job: Job<T>) -> Result<(), SubmitError<T>> {
    if !self.running() {
      return Err(SubmitError { job, full: false });
    }
    let (mutex, ready) = &*self.state;
    let mut state = mutex.lock().unwrap();
    if state.stop || state.outstanding == 3 {
      return Err(SubmitError { job, full: !state.stop });
    }
    state.jobs.push_back(job);
    state.outstanding += 1;
    ready.notify_one();
    Ok(())
  }

  fn running(&mut self) -> bool {
    if self.worker.as_ref().is_some_and(Task::finished)
      && let Err(panic) = self.worker.take().unwrap().join()
    {
      std::panic::resume_unwind(panic);
    }
    self.worker.is_some()
  }

  pub fn take(&mut self) -> io::Result<Option<Completion<T>>> {
    if !self.running() {
      return Err(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "conversion worker terminated",
      ));
    }
    let mut state = self.state.0.lock().unwrap();
    let result = state.completed.pop_front();
    if result.is_some() {
      state.outstanding -= 1;
    }
    Ok(result)
  }
}
impl<T: Input> Drop for Converter<T> {
  fn drop(&mut self) {
    {
      let mut state = self
        .state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
      state.stop = true;
      self.state.1.notify_one();
    }
    if let Some(worker) = self.worker.take()
      && let Err(panic) = worker.join()
    {
      if std::thread::panicking() {
        eprintln!("additional converter panic during unwind");
      } else {
        std::panic::resume_unwind(panic);
      }
    }
    // Queued inputs and completed results retain their frame/buffer
    // leases until the worker stops.
  }
}
