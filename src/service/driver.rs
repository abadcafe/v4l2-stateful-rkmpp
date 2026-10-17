//! module: service::driver; responsibility: bind the scheduler to
//! production channel and decoder contracts. operations: run; entities:
//! none; tests: driver_tests and process acceptance.
use std::io;

use super::worker::{self, Channel, Engine};
use crate::backend::Backend;
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{Batch, Message};
use crate::shutdown::{Shutdown, Signals};
use crate::transport::Session;
impl Channel for Session {
  fn exchange(&mut self, batch: &mut Batch) -> io::Result<()> {
    Session::exchange(self, batch)
  }

  fn register(
    &mut self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<Pool> {
    Session::register(self, direction, epoch, id)
  }

  fn wait(
    &mut self,
    notify: &Notify,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    Session::wait(self, notify, shutdown)
  }
}
impl Engine for Backend {
  fn notification(&self) -> Notify {
    Backend::notification(self)
  }

  fn register(&mut self, pool: Pool) -> io::Result<()> {
    Backend::register(self, pool)
  }

  fn event(&mut self, command: Command) -> io::Result<()> {
    Backend::event(self, command)
  }

  fn step(&mut self) -> io::Result<bool> {
    Backend::step(self)
  }

  fn take_reply(&mut self) -> Option<Message> {
    Backend::take_reply(self)
  }
}
pub(super) fn run(
  mut session: Session,
  shutdown: Shutdown,
) -> Result<(), String> {
  Signals::block_worker().map_err(|e| e.to_string())?;
  let backend = Backend::new().map_err(|e| e.to_string())?;
  worker::run(&mut session, backend, &shutdown).map(|_| ())
}
