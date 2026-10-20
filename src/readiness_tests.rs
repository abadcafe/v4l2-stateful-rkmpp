//! module: readiness_tests; responsibility: define asynchronous
//! descriptor wakeups. operations: tests; entities: none; tests:
//! ready-before-wait, rearm, cancellation.
use std::os::fd::AsFd;
use std::time::Duration;

use crate::notify::Notify;
use crate::readiness::Readable;
#[tokio::test(flavor = "current_thread")]
async fn test_new_preserves_registration_errno() {
  let file = std::fs::File::open("/dev/null").unwrap();
  let error = Readable::new(file.into()).err().unwrap();
  assert_eq!(error.raw_os_error(), Some(libc::EPERM));
  assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_preserves_pending_and_subsequent_wakeups() {
  let notify = Notify::new().unwrap();
  let ready =
    Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
      .unwrap();
  for _ in 0..3 {
    notify.signal().unwrap();
    tokio::time::timeout(Duration::from_secs(1), ready.wait())
      .await
      .unwrap()
      .unwrap();
    assert_eq!(notify.drain().unwrap(), 1);
  }
}
#[tokio::test(flavor = "current_thread")]
async fn test_wait_cancellation_does_not_consume_notification() {
  let notify = Notify::new().unwrap();
  let ready =
    Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
      .unwrap();
  assert!(
    tokio::time::timeout(Duration::from_millis(10), ready.wait())
      .await
      .is_err()
  );
  notify.signal().unwrap();
  ready.wait().await.unwrap();
  assert_eq!(notify.drain().unwrap(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_lease_retains_descriptor_after_readiness_registration_is_dropped()
 {
  use std::io::Read;
  let (stream, mut peer) =
    std::os::unix::net::UnixStream::pair().unwrap();
  peer.set_nonblocking(true).unwrap();
  let ready = Readable::new(stream.into()).unwrap();
  let lease = ready.lease();
  drop(ready);
  assert_eq!(
    peer.read(&mut [0]).unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  drop(lease);
  assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_retains_level_readiness_until_owner_drains_descriptor()
 {
  let notify = Notify::new().unwrap();
  let ready =
    Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
      .unwrap();
  notify.signal().unwrap();
  for _ in 0..3 {
    tokio::time::timeout(Duration::from_secs(1), ready.wait())
      .await
      .unwrap()
      .unwrap();
  }
  notify.drain().unwrap();
  assert!(
    tokio::time::timeout(Duration::from_millis(10), ready.wait())
      .await
      .is_err()
  );
  notify.signal().unwrap();
  ready.wait().await.unwrap();
}
