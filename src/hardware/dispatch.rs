//! module: hardware::dispatch; responsibility: execute one bounded
//! decoder turn. operations: submit, advance, take_reply;
//! entities: Dispatch, Operation; tests: dispatch_tests. Command/reply
//! storage returns with the turn for reuse.
use std::collections::VecDeque;
use std::io;

use crate::backend::Backend;
use crate::pool::Pool;
use crate::protocol::Message;
use crate::protocol::command::Command;
struct Operation {
  command: Command,
  pool: Option<Pool>,
}
#[derive(Default)]
pub(super) struct Dispatch {
  operations: Vec<Operation>,
  replies: VecDeque<Message>,
}
impl Dispatch {
  pub(super) fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()> {
    if self.operations.len() == crate::protocol::MAX_BATCH {
      return Err(io::Error::other("hardware command batch full"));
    }
    self.operations.push(Operation { command, pool });
    Ok(())
  }

  pub(super) fn advance(
    &mut self,
    backend: &mut Backend,
  ) -> io::Result<bool> {
    for operation in self.operations.drain(..) {
      if let Some(pool) = operation.pool {
        backend.register(pool)?;
      }
      backend.event(operation.command)?;
    }
    let mut runnable = false;
    for _ in 0..4 {
      runnable = backend.step()?;
      if !runnable {
        break;
      }
    }
    while let Some(reply) = backend.take_reply() {
      self.replies.push_back(reply);
    }
    Ok(runnable)
  }

  pub(super) fn take_reply(&mut self) -> Option<Message> {
    self.replies.pop_front()
  }
}
