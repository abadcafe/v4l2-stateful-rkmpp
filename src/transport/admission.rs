//! module: transport::admission; responsibility: classify broker ioctl
//! outcomes by fault scope. operations: classify; entities: Admission;
//! tests: admission_tests. Poll/descriptor failures remain fatal
//! errors.
use std::io;

use super::Session;
pub enum Admission {
  Accepted(Session),
  Idle,
  Rejected(io::Error),
  Pressure(io::Error),
}
impl Admission {
  pub(super) fn classify(error: io::Error) -> io::Result<Self> {
    match error.raw_os_error() {
      Some(libc::EINTR | libc::EAGAIN) => Ok(Self::Idle),
      Some(libc::EPIPE) => Ok(Self::Rejected(error)),
      Some(libc::ENOMEM | libc::EMFILE | libc::ENFILE) => {
        Ok(Self::Pressure(error))
      }
      _ => Err(error),
    }
  }
}
