//! module: converter; responsibility: schedule bounded, owned
//! asynchronous GPU conversions. operations: new, submit, take;
//! entities: Converter, Job, Completion, SubmitError, Input.
//! tests: converter_tests; ownership is returned on backpressure or
//! completion, and Drop joins GPU work.
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::{io, thread};

use crate::image::{Image, Plane};
use crate::notify::Notify;
use crate::quantize::Quantizer;
pub trait Input: Send + 'static {
  fn plane(&self) -> Plane<'_>;
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
  worker: Option<thread::JoinHandle<()>>,
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
    let worker = thread::Builder::new()
      .name("nv15-quantize".into())
      .spawn(move || {
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

  pub fn take(&mut self) -> Option<Completion<T>> {
    let mut state = self.state.0.lock().unwrap();
    let result = state.completed.pop_front()?;
    state.outstanding -= 1;
    Some(result)
  }
}
impl<T: Input> Drop for Converter<T> {
  fn drop(&mut self) {
    {
      let mut state = self.state.0.lock().unwrap();
      state.stop = true;
      self.state.1.notify_one();
    }
    if let Some(worker) = self.worker.take()
      && let Err(panic) = worker.join()
    {
      std::panic::resume_unwind(panic);
    }
    // Queued inputs and completed results retain their frame/buffer
    // leases until the worker stops.
  }
}
