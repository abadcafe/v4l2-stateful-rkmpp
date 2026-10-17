//! module: transport_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_open_missing_broker_returns_os_error
//! test_accept_non_broker_file_reports_ioctl_error_and_shutdown_stops_accept
//! test_exchange_validates_header_before_external_ioctl
//! test_wait_notification_drains_wakeup_without_control_readiness
//! test_wait_observes_closed_control_and_drop_closes_owned_channel
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::notify::Notify;
use crate::pool::Direction;
use crate::protocol::{Batch, VERSION};
use crate::shutdown::Signals;
use crate::transport::{Admission, Broker, Session};

#[test]
fn test_open_missing_broker_returns_os_error() {
  assert_eq!(
    Broker::open(Path::new("/nonexistent/v4l2-rkmpp-control"))
      .err()
      .unwrap()
      .kind(),
    std::io::ErrorKind::NotFound
  );
}

#[test]
fn test_accept_non_broker_file_reports_ioctl_error_and_shutdown_stops_accept()
 {
  let broker = Broker::open(Path::new("/dev/null")).unwrap();
  let signals = Signals::install().unwrap();
  assert_eq!(
    broker
      .accept(&signals, &Notify::new().unwrap())
      .err()
      .unwrap()
      .raw_os_error(),
    Some(libc::ENOTTY)
  );
  signals.shutdown().request();
  assert!(matches!(
    broker.accept(&signals, &Notify::new().unwrap()).unwrap(),
    Admission::Idle
  ));
}

#[test]
fn test_exchange_validates_header_before_external_ioctl() {
  let fd: std::os::fd::OwnedFd =
    std::fs::File::open("/dev/null").unwrap().into();
  let session = Session::from(fd);
  assert_eq!(
    session.exchange(&mut Batch::default()).unwrap_err().kind(),
    std::io::ErrorKind::InvalidData
  );
  assert_eq!(
    session
      .exchange(&mut Batch { version: VERSION, ..Default::default() })
      .unwrap_err()
      .raw_os_error(),
    Some(libc::ENOTTY)
  );
  assert_eq!(
    session
      .register(Direction::Output, 1, 1)
      .err()
      .unwrap()
      .raw_os_error(),
    Some(libc::ENOTTY)
  );
}

#[test]
fn test_wait_notification_drains_wakeup_without_control_readiness() {
  let (stream, _peer) = UnixStream::pair().unwrap();
  let session = Session::from(std::os::fd::OwnedFd::from(stream));
  let notify = Notify::new().unwrap();
  let signals = Signals::install().unwrap();
  notify.signal().unwrap();
  assert!(!session.wait(&notify, &signals.shutdown()).unwrap());
  assert!(!notify.wait(Duration::ZERO).unwrap());
  signals.shutdown().request();
  assert!(!session.wait(&notify, &signals.shutdown()).unwrap());
}

#[test]
fn test_wait_observes_closed_control_and_drop_closes_owned_channel() {
  let (stream, peer) = UnixStream::pair().unwrap();
  let session = Session::from(std::os::fd::OwnedFd::from(stream));
  let signals = Signals::install().unwrap();
  drop(peer);
  assert!(
    session.wait(&Notify::new().unwrap(), &signals.shutdown()).unwrap()
  );
  drop(session);
  let (stream, mut peer) = UnixStream::pair().unwrap();
  peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
  let session = Session::from(std::os::fd::OwnedFd::from(stream));
  drop(session);
  assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn test_accept_worker_exit_wakes_idle_broker_without_an_ioctl() {
  use std::os::fd::{FromRawFd, OwnedFd};
  let mut fds = [-1; 2];
  assert_eq!(
    unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) },
    0
  );
  let _reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
  let _writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
  let broker =
    Broker::open(Path::new(&format!("/proc/self/fd/{}", fds[0])))
      .unwrap();
  let signals = Signals::install().unwrap();
  let wake = Notify::new().unwrap();
  let exit = wake.clone();
  let thread = std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(20));
    exit.signal().unwrap();
  });
  let started = std::time::Instant::now();
  assert!(matches!(
    broker.accept(&signals, &wake).unwrap(),
    Admission::Idle
  ));
  assert!(started.elapsed() < Duration::from_secs(1));
  assert!(!wake.wait(Duration::ZERO).unwrap());
  thread.join().unwrap();
}
