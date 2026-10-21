//! module: dma_buf::access; responsibility: enforce CPU access
//! ownership. operations: Owner, State, Lease::new/finish; entities:
//! State, Lease; tests: access_tests. Aliases share an opaque state;
//! synchronization is supplied by the storage owner, independent of fd
//! representation.
use std::io;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Default)]
pub(crate) struct State(AtomicU8);
const IDLE: u8 = 0;
const ACTIVE: u8 = 1;
const FAILED: u8 = 2;
pub(crate) trait Owner {
  fn state(&self) -> &State;
  fn synchronize(&self, start: bool, write: bool) -> io::Result<()>;
}
pub(crate) struct Lease<T: Owner> {
  owner: T,
  write: bool,
  active: bool,
}
fn synchronize(
  owner: &impl Owner,
  start: bool,
  write: bool,
) -> io::Result<()> {
  loop {
    match owner.synchronize(start, write) {
      Err(error)
        if matches!(
          error.raw_os_error(),
          Some(libc::EINTR | libc::EAGAIN)
        ) =>
      {
        continue;
      }
      result => return result,
    }
  }
}
impl<T: Owner> Lease<T> {
  pub(super) fn new(owner: T, write: bool) -> io::Result<Self> {
    match owner.state().0.compare_exchange(
      IDLE,
      ACTIVE,
      Ordering::AcqRel,
      Ordering::Acquire,
    ) {
      Ok(_) => {}
      Err(FAILED) => {
        return Err(io::Error::from_raw_os_error(libc::EIO));
      }
      Err(_) => {
        return Err(io::Error::new(
          io::ErrorKind::WouldBlock,
          "DMA buffer already has a CPU access lease",
        ));
      }
    }
    if let Err(error) = synchronize(&owner, true, write) {
      owner.state().0.store(IDLE, Ordering::Release);
      return Err(error);
    }
    Ok(Self { owner, write, active: true })
  }

  fn end(&self) -> io::Result<()> {
    let result = synchronize(&self.owner, false, self.write);
    // A failed END does not establish device visibility. Never let
    // another alias mistake that storage for a successfully released
    // CPU lease, including when Drop cannot return the error.
    self.owner.state().0.store(
      if result.is_ok() { IDLE } else { FAILED },
      Ordering::Release,
    );
    result
  }

  pub(crate) fn finish(mut self) -> io::Result<()> {
    self.active = false;
    self.end()
  }
}
impl<T: Owner> Drop for Lease<T> {
  fn drop(&mut self) {
    if self.active {
      let _ = self.end();
    }
  }
}
