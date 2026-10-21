//! module: notify; responsibility: an owned, clonable eventfd wakeup.
//! operations: new, signal, drain; entities: Notify; tests:
//! notify_tests.
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub struct Notify(Arc<OwnedFd>);
impl Notify {
  pub fn new() -> io::Result<Self> {
    // SAFETY: no pointers; a successful eventfd is uniquely owned.
    let fd = unsafe {
      libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC)
    };
    if fd < 0 {
      return Err(io::Error::last_os_error());
    }
    Ok(Self(Arc::new(unsafe { OwnedFd::from_raw_fd(fd) })))
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
    let mut total = 0u64;
    loop {
      let mut value = 0u64;
      // SAFETY: value is an aligned writable eight-byte destination.
      let result = unsafe {
        libc::read(
          self.0.as_raw_fd(),
          (&mut value as *mut u64).cast(),
          8,
        )
      };
      if result == 8 {
        total = total.saturating_add(value);
        continue;
      }
      let error = io::Error::last_os_error();
      if error.kind() == io::ErrorKind::WouldBlock {
        return Ok(total);
      }
      if error.kind() != io::ErrorKind::Interrupted {
        return Err(error);
      }
    }
  }
}
impl AsFd for Notify {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.as_fd()
  }
}
