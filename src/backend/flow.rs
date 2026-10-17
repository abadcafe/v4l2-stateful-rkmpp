//! module: backend::flow; responsibility: preserve sequence semantics
//! across drains. operations: begin/abort/complete/resume,
//! running/waiting; entities: End, Completion, State, Flow; tests:
//! flow_tests and service lifecycle acceptance.
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
  Explicit,
  Natural,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Completion {
  Notify,
  Continue { reset: bool },
}
#[derive(Default)]
enum State {
  #[default]
  Running,
  Waiting(End),
  Aborting(End),
  Complete(End),
}
#[derive(Default)]
pub(super) struct Flow(State);
impl Flow {
  pub(super) fn running(&self) -> bool {
    matches!(self.0, State::Running)
  }

  pub(super) fn waiting(&self) -> bool {
    matches!(self.0, State::Waiting(_) | State::Aborting(_))
  }

  pub(super) fn begin(&mut self, end: End) -> io::Result<()> {
    if !self.running() {
      return Err(io::Error::other("overlapping decode drain"));
    }
    self.0 = State::Waiting(end);
    Ok(())
  }

  pub(super) fn abort(&mut self) -> bool {
    match self.0 {
      State::Waiting(end) | State::Aborting(end) => {
        self.0 = State::Aborting(end);
        false
      }
      State::Complete(end) => {
        self.0 = State::Running;
        end == End::Natural
      }
      State::Running => false,
    }
  }

  pub(super) fn complete(&mut self) -> io::Result<Completion> {
    match self.0 {
      State::Waiting(end) => {
        self.0 = State::Complete(end);
        Ok(Completion::Notify)
      }
      State::Aborting(end) => {
        self.0 = State::Running;
        Ok(Completion::Continue { reset: end == End::Natural })
      }
      _ => Err(io::Error::other("completion without an active drain")),
    }
  }

  pub(super) fn resume(&mut self) -> io::Result<bool> {
    match self.0 {
      State::Aborting(_) | State::Running => Ok(false),
      State::Complete(end) => {
        self.0 = State::Running;
        Ok(end == End::Natural)
      }
      State::Waiting(_) => {
        Err(io::Error::other("resume before drain completion"))
      }
    }
  }
}
