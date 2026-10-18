//! module: service::driver_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_run_external_channel_error_is_propagated_through_real_adapter
use super::driver;
use crate::shutdown::Signals;
use crate::transport::Session;
#[tokio::test(flavor = "current_thread")]
async fn test_run_external_channel_error_is_propagated_through_real_adapter()
 {
  let signals = Signals::install().unwrap();
  let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
  let fd: std::os::fd::OwnedFd = stream.into();
  let hardware = crate::hardware::Hardware::start();
  assert!(
    driver::run(
      Session::new(fd).unwrap(),
      signals.shutdown(),
      hardware.client()
    )
    .await
    .unwrap_err()
    .contains("exchange")
  );
  hardware.close().await.unwrap();
}
