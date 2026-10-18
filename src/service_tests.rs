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
