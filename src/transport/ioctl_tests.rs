//! module: transport::ioctl_tests; responsibility: define borrowed ABI
//! transaction boundaries. operations: tests; entities: none.
use std::fs::File;
use std::os::fd::OwnedFd;

use super::ioctl;
use crate::descriptor::Lease;
use crate::protocol::{Batch, VERSION};

#[test]
fn test_exchange_failure_preserves_the_callers_owned_request() {
  let lease =
    Lease::new(OwnedFd::from(File::open("/dev/null").unwrap()));
  let mut batch = Box::new(Batch {
    version: VERSION,
    reply_count: 1,
    ..Default::default()
  });
  batch.replies[0].epoch = 42;
  batch.events[15].epoch = 99;
  batch.results[15] = -7;
  let error = ioctl::exchange(lease, &mut batch).unwrap_err();
  assert_eq!(error.raw_os_error(), Some(libc::ENOTTY));
  assert_eq!(batch.reply_count, 1);
  assert_eq!(batch.replies[0].epoch, 42);
  assert_eq!(batch.events[15].epoch, 99);
  assert_eq!(batch.results[15], -7);
}

#[test]
fn test_exchange_invalid_header_is_rejected_before_ioctl() {
  let lease =
    Lease::new(OwnedFd::from(File::open("/dev/null").unwrap()));
  let mut batch = Box::<Batch>::default();
  let error = ioctl::exchange(lease, &mut batch).unwrap_err();
  assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}
