//! module: notify; responsibility: an owned, clonable eventfd wakeup.
//! operations: new, signal, drain, listen, Listener::wait; entities:
//! Notify, Listener; tests: notify_tests and service resource tests.
//! A listener shares the descriptor owner and always drains the
//! counter it observes. Registration never duplicates the descriptor.
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use crate::descriptor::Lease;
use crate::readiness::Readable;
#[derive(Clone, Debug)]
pub struct Notify(Lease);
pub struct Listener {
  notify: Notify,
  ready: Readable,
}
impl Notify {
  pub fn new() -> io::Result<Self> {
    // SAFETY: no pointers; a successful eventfd is uniquely owned.
    let fd = unsafe {
      libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC)
    };
    if fd < 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(Self(Lease::new(unsafe { OwnedFd::from_raw_fd(fd) })))
  }

  pub fn listen(&self) -> io::Result<Listener> {
    Ok(Listener {
      ready: Readable::from_lease(self.0.clone())?,
      notify: self.clone(),
    })
  }

  pub fn signal(&self) -> io::Result<()> {
    let value = 1u64;
    loop {
      // SAFETY: value remains valid for the synchronous write.
      let result = unsafe {
        libc::write(
          self.0.as_raw_fd(),
          (&value as *const u64).cast(),
          8,
        )
      };
      if result == 8 {
        return Ok(());
      }
      let error = io::Error::last_os_error();
      if error.kind() == io::ErrorKind::WouldBlock {
        return Ok(());
      }
      if error.kind() != io::ErrorKind::Interrupted {
        return Err(error);
      }
    }
  }

  pub fn drain(&self) -> io::Result<u64> {
    drain(self.as_fd())
  }
}
impl Listener {
  pub async fn wait(&self) -> io::Result<u64> {
    self.ready.wait().await?;
    self.notify.drain()
  }
}
fn drain(fd: BorrowedFd<'_>) -> io::Result<u64> {
  loop {
    let mut value = 0u64;
    // SAFETY: value is an aligned writable eight-byte destination.
    let result = unsafe {
      libc::read(fd.as_raw_fd(), (&mut value as *mut u64).cast(), 8)
    };
    if result == 8 {
      // This eventfd has no EFD_SEMAPHORE: one read atomically drains
      // the entire counter. Leave later notifications for the next
      // scheduler turn, instead of chasing concurrent producers here.
      return Ok(value);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
      return Ok(0);
    }
    if error.kind() != io::ErrorKind::Interrupted {
      return Err(error);
    }
  }
}
impl AsFd for Notify {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.as_fd()
  }
}
