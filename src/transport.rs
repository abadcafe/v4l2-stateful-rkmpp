//! module: transport; responsibility: transfer owned resources over the
//! published bridge ABI. operations: broker accept, session
//! exchange/register/wait; entities: Broker, Session; registered
//! resources use the pool contract. tests: transport_tests,
//! tests/kernel and tests/service/test_recovery; no socket fd
//! transport.
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::notify::Notify;
use crate::pool::{Direction, Pool as RegisteredPool};
use crate::protocol::registration::Pool;
use crate::protocol::{Batch, VERSION};
use crate::readiness::Readable;
use crate::shutdown::Shutdown;
mod admission;
#[cfg(test)]
mod admission_tests;
pub use admission::Admission;
pub struct Broker {
  ready: Readable,
}
pub struct Session {
  ready: Readable,
}
mod registration;
#[cfg(test)]
mod registration_tests;

impl Session {
  pub fn lease(&self) -> crate::descriptor::Lease {
    self.ready.lease()
  }

  pub fn new(fd: OwnedFd) -> io::Result<Self> {
    let ready = Readable::new(fd)?;
    Ok(Self { ready })
  }
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
    Ok(Self { ready })
  }

  pub async fn accept(&self) -> io::Result<Admission<Session>> {
    self.ready.wait().await?;
    const ACCEPT: libc::c_ulong = ((b'R' as libc::c_ulong) << 8) | 0x80;
    let fd =
      unsafe { libc::ioctl(self.ready.as_fd().as_raw_fd(), ACCEPT) };
    if fd < 0 {
      return Admission::classify(io::Error::last_os_error());
    }
    // SAFETY: ACCEPT installs and returns a fresh session fd in this
    // process.
    Ok(Admission::Accepted(Session::new(unsafe {
      OwnedFd::from_raw_fd(fd)
    })?))
  }
}
impl Session {
  pub fn exchange(&self, batch: &mut Batch) -> io::Result<()> {
    batch.validate()?;
    let submitted = batch.reply_count;
    // SAFETY: Batch matches bridge_uapi.h and remains writable until
    // ioctl completes.
    if unsafe {
      libc::ioctl(
        self.ready.as_fd().as_raw_fd(),
        Batch::IOCTL,
        &mut *batch,
      )
    } < 0
    {
      return Err(io::Error::last_os_error());
    }
    batch.validate()?;
    // Each submitted reply has exactly one zero-or-negative errno
    // result. Reject malformed results here, before scheduling events
    // or converting a negative status to its positive errno.
    if batch.reply_count != submitted
      || batch.results[..submitted as usize]
        .iter()
        .any(|status| !(-4095..=0).contains(status))
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid exchange response",
      ));
    }
    Ok(())
  }

  pub fn register(
    &self,
    direction: Direction,
    epoch: u64,
    id: u32,
  ) -> io::Result<RegisteredPool> {
    let mut pool = Pool {
      version: VERSION,
      kind: direction as u32,
      epoch,
      pool_id: id,
      ..Default::default()
    };
    // SAFETY: REGISTER_POOL installs fresh fds only on successful
    // copyout.
    if unsafe {
      libc::ioctl(
        self.ready.as_fd().as_raw_fd(),
        Pool::IOCTL,
        &mut pool,
      )
    } < 0
    {
      return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful REGISTER_POOL above transferred unique
    // kernel-exported descriptors into this response. No other Rust
    // owner has been constructed for them.
    unsafe { registration::adopt(pool, direction, epoch, id) }
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
