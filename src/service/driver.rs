//! module: service::driver; responsibility: bind the scheduler to
//! production channel and decoder contracts. operations: run; entities:
//! none; tests: driver_tests and process acceptance.
use std::io;

use super::scheduler::{self, Channel, Engine};
use crate::hardware::{Client, Engine as HardwareEngine};
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{Batch, Message};
use crate::readiness::Readable;
use crate::shutdown::Shutdown;
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

  async fn wait(
    &mut self,
    notify: &Notify,
    notification: &Readable,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    Session::wait(self, notify, notification, shutdown).await
  }
}
impl Engine for HardwareEngine {
  async fn close(self) -> io::Result<()> {
    HardwareEngine::close(self).await
  }

  fn notification(&self) -> Notify {
    HardwareEngine::notification(self)
  }

  fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()> {
    HardwareEngine::submit(self, command, pool)
  }

  async fn advance(&mut self) -> io::Result<bool> {
    HardwareEngine::advance(self).await
  }

  async fn wait(&self) -> io::Result<()> {
    HardwareEngine::wait(self).await
  }

  fn take_reply(&mut self) -> Option<Message> {
    HardwareEngine::take_reply(self)
  }
}
pub(super) async fn run(
  mut session: Session,
  shutdown: Shutdown,
  client: Client,
) -> Result<(), String> {
  let backend =
    client.open(session.lease()).await.map_err(|e| e.to_string())?;
  scheduler::run(&mut session, backend, &shutdown).await.map(|_| ())
}
