//! module: diagnostics::status_tests; responsibility: define self
//! resource snapshots. operations: Rust test cases; entities: none;
//! tests: snapshot schema, stable identity and shutdown. Exact process
//! resource counts are covered by tests/service/test_diagnostics.py.
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

use super::status::Status;
use crate::shutdown::Signals;

#[test]
fn test_snapshot_reports_schema_identity_and_monotonic_cpu_time() {
  let control = File::open("/dev/null").unwrap();
  let signals = Signals::install().unwrap();
  let (client, _peer) = UnixStream::pair().unwrap();
  let status =
    Status::current(control.as_fd(), &signals.shutdown()).unwrap();
  let snapshot: serde_json::Value =
    serde_json::from_slice(&status.snapshot(client.as_fd()).unwrap())
      .unwrap();
  assert_eq!(snapshot["protocol"], 1);
  assert_eq!(snapshot["state"], "ready");
  assert_eq!(snapshot["identity"]["pid"], std::process::id());
  assert!(snapshot["fds"].as_u64().unwrap() > 0);
  assert!(snapshot["threads"].as_u64().unwrap() > 0);
  assert!(snapshot["sessions"].as_array().unwrap().is_empty());
  let later: serde_json::Value =
    serde_json::from_slice(&status.snapshot(client.as_fd()).unwrap())
      .unwrap();
  assert_eq!(snapshot["identity"], later["identity"]);
  assert!(
    later["cpu_seconds"].as_f64().unwrap()
      >= snapshot["cpu_seconds"].as_f64().unwrap()
  );
}

#[test]
fn test_snapshot_tracks_shutdown_and_borrowed_control_identity() {
  let control = File::open("/dev/null").unwrap();
  let metadata = control.metadata().unwrap();
  let signals = Signals::install().unwrap();
  let (client, _peer) = UnixStream::pair().unwrap();
  let status =
    Status::current(control.as_fd(), &signals.shutdown()).unwrap();
  signals.shutdown().request();
  let snapshot: serde_json::Value =
    serde_json::from_slice(&status.snapshot(client.as_fd()).unwrap())
      .unwrap();
  use std::os::unix::fs::MetadataExt;
  assert_eq!(snapshot["state"], "stopping");
  assert_eq!(
    snapshot["identity"]["control"]["raw_device"],
    metadata.rdev()
  );
  assert_eq!(snapshot["identity"]["control"]["inode"], metadata.ino());
  assert_eq!(snapshot["identity"]["control"]["device"], metadata.dev());
}
