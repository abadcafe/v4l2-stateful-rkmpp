//! module: notify_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_signal_accumulates_and_drains_without_polling
use std::os::fd::AsFd;
use std::time::Duration;

use crate::notify::Notify;

#[test]
fn test_signal_accumulates_and_drains_without_polling() {
  let event = Notify::new().unwrap();
  assert!(!event.wait(Duration::ZERO).unwrap());
  event.signal().unwrap();
  event.clone().signal().unwrap();
  assert!(event.wait(Duration::from_secs(1)).unwrap());
  assert_eq!(event.drain().unwrap(), 2);
  assert!(!event.wait(Duration::ZERO).unwrap());
  assert_eq!(event.drain().unwrap(), 0);
  let _ = event.as_fd();
}
