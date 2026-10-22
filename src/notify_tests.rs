//! module: notify_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_signal_accumulates_and_drains_without_polling
use std::os::fd::{AsFd, AsRawFd};

use crate::notify::Notify;

#[tokio::test(flavor = "current_thread")]
async fn test_listen_duplicate_registration_rejects_and_drop_allows_recreation()
 {
  let notify = Notify::new().unwrap();
  let listener = notify.listen().unwrap();
  assert_eq!(
    notify.listen().err().unwrap().kind(),
    std::io::ErrorKind::AlreadyExists
  );
  notify.signal().unwrap();
  assert_eq!(listener.wait().await.unwrap(), 1);
  drop(listener);
  let listener = notify.listen().unwrap();
  notify.signal().unwrap();
  assert_eq!(listener.wait().await.unwrap(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_listen_retains_and_drains_pending_signals_after_sender_drop()
 {
  let notify = Notify::new().unwrap();
  let listener = notify.listen().unwrap();
  notify.signal().unwrap();
  notify.signal().unwrap();
  drop(notify);
  assert_eq!(listener.wait().await.unwrap(), 2);
  assert!(
    tokio::time::timeout(
      std::time::Duration::from_millis(10),
      listener.wait()
    )
    .await
    .is_err()
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_listen_cancelled_wait_preserves_the_next_wakeup() {
  let notify = Notify::new().unwrap();
  let listener = notify.listen().unwrap();
  assert!(
    tokio::time::timeout(
      std::time::Duration::from_millis(10),
      listener.wait()
    )
    .await
    .is_err()
  );
  notify.signal().unwrap();
  assert_eq!(
    tokio::time::timeout(
      std::time::Duration::from_secs(1),
      listener.wait()
    )
    .await
    .unwrap()
    .unwrap(),
    1
  );
  notify.signal().unwrap();
  drop(listener);
  assert_eq!(notify.drain().unwrap(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_listen_observes_only_its_own_counter() {
  let first = Notify::new().unwrap();
  let second = Notify::new().unwrap();
  let first_listener = first.listen().unwrap();
  let second_listener = second.listen().unwrap();
  first.signal().unwrap();
  assert_eq!(first_listener.wait().await.unwrap(), 1);
  assert!(
    tokio::time::timeout(
      std::time::Duration::from_millis(10),
      second_listener.wait()
    )
    .await
    .is_err()
  );
  second.signal().unwrap();
  assert_eq!(second_listener.wait().await.unwrap(), 1);
}

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
