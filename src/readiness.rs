//! module: readiness; responsibility: await owned descriptor readiness
//! without blocking the runtime. operations: new, wait, lease;
//! entities: Readable; tests: readiness_tests. Waiting
//! never consumes data from the descriptor.
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};

use tokio::io::unix::AsyncFd;

use crate::descriptor::Lease as DescriptorLease;
pub struct Readable(AsyncFd<DescriptorLease>);
impl Readable {
  pub fn new(owned: OwnedFd) -> io::Result<Self> {
    Self::from_lease(DescriptorLease::new(owned))
  }

  pub fn from_lease(lease: DescriptorLease) -> io::Result<Self> {
    // SAFETY: owned is a stable descriptor retained until
    // deregistration; all reads remain nonblocking and are
    // performed by the descriptor owner.
    let registered = unsafe {
      AsyncFd::register_with_interest(
        lease,
        tokio::io::Interest::READABLE,
      )
    }?;
    Ok(Self(registered))
  }

  pub fn lease(&self) -> DescriptorLease {
    self.0.get_ref().clone()
  }

  pub async fn wait(&self) -> io::Result<()> {
    loop {
      let mut ready = self.0.readable().await?;
      let mut poll = libc::pollfd {
        fd: self.0.get_ref().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      };
      // A readiness-only consumer cannot report EAGAIN through try_io.
      // Validate the cached edge while retaining its generation guard:
      // do not discard an edge while the producer queue remains
      // nonempty.
      let result = unsafe { libc::poll(&mut poll, 1, 0) };
      if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
          continue;
        }
        return Err(error);
      }
      if poll.revents != 0 {
        return Ok(());
      }
      ready.clear_ready();
    }
  }
}

impl AsFd for Readable {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.get_ref().as_fd()
  }
}
