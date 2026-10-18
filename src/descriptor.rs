//! module: descriptor; responsibility: retain a shared kernel
//! descriptor. operations: new, clone, AsFd, AsRawFd; entities: Lease;
//! tests: descriptor_tests.
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub struct Lease(Arc<OwnedFd>);
impl Lease {
  pub fn new(fd: OwnedFd) -> Self {
    Self(Arc::new(fd))
  }
}
impl AsFd for Lease {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.as_fd()
  }
}
impl AsRawFd for Lease {
  fn as_raw_fd(&self) -> RawFd {
    self.0.as_raw_fd()
  }
}
