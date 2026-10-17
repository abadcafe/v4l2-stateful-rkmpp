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
use crate::shutdown::{Shutdown, Signals};
mod admission;
#[cfg(test)]
mod admission_tests;
pub use admission::Admission;
pub struct Broker(OwnedFd);
pub struct Session(OwnedFd);
mod registration;
#[cfg(test)]
mod registration_tests;

impl From<OwnedFd> for Session {
  fn from(fd: OwnedFd) -> Self {
    Self(fd)
  }
}
impl AsFd for Broker {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.as_fd()
  }
}
impl Broker {
  pub fn open(path: &Path) -> io::Result<Self> {
    Ok(Self(
      OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?
        .into(),
    ))
  }

  pub fn accept(
    &self,
    signals: &Signals,
    workers: &Notify,
  ) -> io::Result<Admission> {
    let mut fds = [
      libc::pollfd {
        fd: self.0.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: signals.as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: workers.as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: signals.shutdown().as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
    ];
    if unsafe {
      libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1)
    } < 0
    {
      let error = io::Error::last_os_error();
      return if error.kind() == io::ErrorKind::Interrupted {
        Ok(Admission::Idle)
      } else {
        Err(error)
      };
    }
    if fds[1].revents & libc::POLLIN != 0 {
      signals.consume()?;
      return Ok(Admission::Idle);
    }
    if signals.shutdown().stopping() {
      return Ok(Admission::Idle);
    }
    if fds[2].revents & libc::POLLIN != 0 {
      workers.drain()?;
      return Ok(Admission::Idle);
    }
    const ACCEPT: libc::c_ulong = ((b'R' as libc::c_ulong) << 8) | 0x80;
    let fd = unsafe { libc::ioctl(self.0.as_raw_fd(), ACCEPT) };
    if fd < 0 {
      return Admission::classify(io::Error::last_os_error());
    }
    // SAFETY: ACCEPT installs and returns a fresh session fd in this
    // process.
    Ok(Admission::Accepted(Session::from(unsafe {
      OwnedFd::from_raw_fd(fd)
    })))
  }
}
impl Session {
  pub fn exchange(&self, batch: &mut Batch) -> io::Result<()> {
    batch.validate()?;
    // SAFETY: Batch matches bridge_uapi.h and remains writable until
    // ioctl completes.
    if unsafe {
      libc::ioctl(self.0.as_raw_fd(), Batch::IOCTL, &mut *batch)
    } < 0
    {
      return Err(io::Error::last_os_error());
    }
    batch.validate()
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
      libc::ioctl(self.0.as_raw_fd(), Pool::IOCTL, &mut pool)
    } < 0
    {
      return Err(io::Error::last_os_error());
    }
    registration::adopt(pool, direction, epoch, id)
  }

  pub fn wait(
    &self,
    notify: &Notify,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    let mut fds = [
      libc::pollfd {
        fd: self.0.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: notify.as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: shutdown.as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
    ];
    if unsafe { libc::poll(fds.as_mut_ptr(), 3, -1) } < 0 {
      return Err(io::Error::last_os_error());
    }
    if fds[1].revents & libc::POLLIN != 0 {
      notify.drain()?;
    }
    Ok(fds[0].revents != 0)
  }
}
