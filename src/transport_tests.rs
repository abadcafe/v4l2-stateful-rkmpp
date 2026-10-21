//! module: transport_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_open_missing_broker_returns_os_error
//! test_accept_non_broker_file_reports_ioctl_error_and_shutdown_stops_accept
//! test_exchange_validates_header_before_external_ioctl
//! test_wait_notification_drains_wakeup_without_control_readiness
//! test_wait_observes_closed_control_and_drop_closes_owned_channel
use std::io::Read;
use std::num::NonZeroUsize;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::execution::{Limits, Pool};
use crate::notify::Notify;
fn executor() -> Pool<()> {
  Pool::start(
    "transport-test",
    Limits {
      workers: NonZeroUsize::new(1).unwrap(),
      backlog: NonZeroUsize::new(4).unwrap(),
    },
    || (),
    |_| {},
  )
  .unwrap()
}
use crate::pool::Direction;
use crate::protocol::{Batch, VERSION};
use crate::readiness::Readable;
use crate::shutdown::Signals;
use crate::transport::{Broker, Session};

#[tokio::test(flavor = "current_thread")]
async fn test_open_missing_broker_returns_os_error() {
  assert_eq!(
    Broker::open(Path::new("/nonexistent/v4l2-rkmpp-control"))
      .err()
      .unwrap()
      .kind(),
    std::io::ErrorKind::NotFound
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_open_non_pollable_file_reports_error() {
  assert!(Broker::open(Path::new("/dev/null")).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn test_exchange_validates_header_before_external_ioctl() {
  let executor = executor();
  let (stream, _peer) = UnixStream::pair().unwrap();
  let fd: std::os::fd::OwnedFd = stream.into();
  let mut session = Session::new(fd, executor.router().lane()).unwrap();
  assert_eq!(
    session.exchange(&mut Batch::default()).await.unwrap_err().kind(),
    std::io::ErrorKind::InvalidData
  );
  assert_eq!(
    session
      .exchange(&mut Batch { version: VERSION, ..Default::default() })
      .await
      .unwrap_err()
      .raw_os_error(),
    Some(libc::ENOTTY)
  );
  assert_eq!(
    session
      .register(Direction::Output, 1, 1)
      .await
      .err()
      .unwrap()
      .raw_os_error(),
    Some(libc::ENOTTY)
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_exchange_cancelled_wait_retains_request_and_reuses_next_transaction()
 {
  let executor = executor();
  let lane = executor.router().lane();
  let (release, blocked) = std::sync::mpsc::channel();
  let (started, running) = tokio::sync::oneshot::channel();
  let blocker = lane
    .schedule(|| {
      move |_| {
        started.send(()).unwrap();
        blocked.recv().unwrap();
        Ok(())
      }
    })
    .await
    .unwrap();
  running.await.unwrap();
  let (stream, _peer) = UnixStream::pair().unwrap();
  let mut session = Session::new(stream.into(), lane).unwrap();
  let mut batch =
    Batch { version: VERSION, reply_count: 1, ..Default::default() };
  batch.replies[0].count = 7;
  assert!(
    tokio::time::timeout(
      Duration::from_millis(10),
      session.exchange(&mut batch)
    )
    .await
    .is_err()
  );
  batch.replies[0].count = 8;
  assert_eq!(
    session.exchange(&mut batch).await.unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  batch.replies[0].count = 7;
  batch.reply_count = 0;
  assert_eq!(
    session.exchange(&mut batch).await.unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  batch.reply_count = 1;
  release.send(()).unwrap();
  blocker.await.unwrap();
  assert_eq!(
    session.exchange(&mut batch).await.unwrap_err().raw_os_error(),
    Some(libc::ENOTTY)
  );
  batch.reply_count = 0;
  assert_eq!(
    session.exchange(&mut batch).await.unwrap_err().raw_os_error(),
    Some(libc::ENOTTY)
  );
  drop(session);
  executor.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_notification_drains_wakeup_without_control_readiness()
 {
  let executor = executor();
  let (stream, _peer) = UnixStream::pair().unwrap();
  let session = Session::new(
    std::os::fd::OwnedFd::from(stream),
    executor.router().lane(),
  )
  .unwrap();
  let notify = Notify::new().unwrap();
  let signals = Signals::install().unwrap();
  notify.signal().unwrap();
  assert!(
    !session
      .wait(
        &notify,
        &Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
          .unwrap(),
        &signals.shutdown()
      )
      .await
      .unwrap()
  );
  assert!(!wait_wakeup(&notify, Duration::ZERO));
  signals.shutdown().request();
  assert!(
    !session
      .wait(
        &notify,
        &Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
          .unwrap(),
        &signals.shutdown()
      )
      .await
      .unwrap()
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_observes_closed_control_and_drop_closes_owned_channel()
 {
  let executor = executor();
  let (stream, peer) = UnixStream::pair().unwrap();
  let session = Session::new(
    std::os::fd::OwnedFd::from(stream),
    executor.router().lane(),
  )
  .unwrap();
  let signals = Signals::install().unwrap();
  drop(peer);
  assert!({
    let notify = Notify::new().unwrap();
    session
      .wait(
        &notify,
        &Readable::new(notify.as_fd().try_clone_to_owned().unwrap())
          .unwrap(),
        &signals.shutdown(),
      )
      .await
      .unwrap()
  });
  drop(session);
  let (stream, mut peer) = UnixStream::pair().unwrap();
  peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
  let session = Session::new(
    std::os::fd::OwnedFd::from(stream),
    executor.router().lane(),
  )
  .unwrap();
  drop(session);
  assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

fn wait_wakeup(
  notify: &crate::notify::Notify,
  timeout: Duration,
) -> bool {
  use std::os::fd::{AsFd, AsRawFd};
  let mut poll = libc::pollfd {
    fd: notify.as_fd().as_raw_fd(),
    events: libc::POLLIN,
    revents: 0,
  };
  // SAFETY: one live descriptor and one writable native poll record.
  assert!(
    unsafe {
      libc::poll(&mut poll, 1, timeout.as_millis().try_into().unwrap())
    } >= 0
  );
  poll.revents & libc::POLLIN != 0
}
