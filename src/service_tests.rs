//! module: service_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_serve_missing_control_reports_path_and_open_error
//! test_serve_non_broker_file_exits_on_accept_error
use std::path::Path;

use crate::service;

#[tokio::test(flavor = "current_thread")]
async fn test_serve_missing_control_reports_path_and_open_error() {
  let error = service::serve(
    Path::new("/nonexistent/v4l2-rkmpp-control"),
    Path::new("/nonexistent/rkmpp-status"),
  )
  .await
  .unwrap_err();
  assert!(
    error.contains("/nonexistent/v4l2-rkmpp-control")
      && error.contains("open control")
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_serve_non_broker_file_exits_on_accept_error() {
  let socket = std::env::temp_dir()
    .join(format!("rkmpp-service-test-{}", std::process::id()));
  let error =
    service::serve(Path::new("/dev/null"), &socket).await.unwrap_err();
  assert!(error.contains("open control"));
  assert!(!socket.exists());
}

use crate::shutdown::Signals;
use crate::transport::Session;
#[tokio::test(flavor = "current_thread")]
async fn test_run_external_channel_error_is_propagated_through_real_adapter()
 {
  let signals = Signals::install().unwrap();
  let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
  let fd: std::os::fd::OwnedFd = stream.into();
  let (hardware, client) = crate::hardware::Hardware::start().unwrap();
  let transport = crate::execution::Pool::start(
    "service-test",
    crate::execution::Limits {
      workers: std::num::NonZeroUsize::new(1).unwrap(),
      backlog: std::num::NonZeroUsize::new(4).unwrap(),
    },
    || (),
    |_| {},
  )
  .unwrap();
  assert!(
    service::run_session(
      Session::new(fd, transport.router().lane()).unwrap(),
      signals.shutdown(),
      client.clone()
    )
    .await
    .unwrap_err()
    .contains("exchange")
  );
  hardware.close().await.unwrap();
  transport.close().await.unwrap();
}
