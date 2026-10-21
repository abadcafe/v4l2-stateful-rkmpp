//! module: notify_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_signal_accumulates_and_drains_without_polling
use std::os::fd::{AsFd, AsRawFd};

use crate::notify::Notify;

#[test]
fn test_signal_accumulates_and_drains_without_polling() {
  let event = Notify::new().unwrap();
  assert!(!readable(&event));
  event.signal().unwrap();
  event.clone().signal().unwrap();
  assert!(readable(&event));
  assert_eq!(event.drain().unwrap(), 2);
  assert!(!readable(&event));
  assert_eq!(event.drain().unwrap(), 0);
  let _ = event.as_fd();
}

fn readable(event: &Notify) -> bool {
  let mut poll = libc::pollfd {
    fd: event.as_fd().as_raw_fd(),
    events: libc::POLLIN,
    revents: 0,
  };
  // SAFETY: one initialized record, retaining the public descriptor
  // borrow.
  assert!(unsafe { libc::poll(&mut poll, 1, 0) } >= 0);
  poll.revents & libc::POLLIN != 0
}
