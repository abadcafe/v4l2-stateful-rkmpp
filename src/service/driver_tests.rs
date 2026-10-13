//! module: service::driver_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_run_external_channel_error_is_propagated_through_real_adapter
use super::driver;
use crate::shutdown::Signals;
use crate::transport::Session;
#[test]
fn test_run_external_channel_error_is_propagated_through_real_adapter()
{
  let signals = Signals::install().unwrap();
  let fd: std::os::fd::OwnedFd =
    std::fs::File::open("/dev/null").unwrap().into();
  assert!(
    driver::run(Session::from(fd), signals.shutdown())
      .unwrap_err()
      .contains("exchange")
  );
}
