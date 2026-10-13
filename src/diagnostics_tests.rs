//! module: diagnostics_tests; responsibility: define read-only endpoint
//! lifetime. operations: Rust test cases; entities: none; tests: query,
//! ownership, cleanup.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use crate::diagnostics::Endpoint;
use crate::shutdown::Signals;

fn path(name: &str) -> PathBuf {
  std::env::temp_dir()
    .join(format!("rkmpp-status-{}-{name}", std::process::id()))
}

fn request(path: &std::path::Path, body: &[u8]) -> serde_json::Value {
  let mut client = UnixStream::connect(path).unwrap();
  client.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
  client.write_all(body).unwrap();
  let mut response = String::new();
  client.read_to_string(&mut response).unwrap();
  serde_json::from_str(&response).unwrap()
}

#[test]
fn test_open_reports_self_identity_resources_and_video_socket_permissions()
 {
  let path = path("query");
  let file = File::open("/dev/null").unwrap();
  let signals = Signals::install().unwrap();
  let endpoint =
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).unwrap();
  assert!(fs::metadata(&path).unwrap().file_type().is_socket());
  assert_eq!(
    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
    0o660
  );
  let first = request(&path, b"snapshot\n");
  assert_eq!(first["protocol"], 1);
  assert_eq!(first["identity"]["pid"], std::process::id());
  assert_eq!(first["identity"]["uid"], unsafe { libc::geteuid() });
  assert_eq!(first["state"], "ready");
  assert!(first["fds"].as_u64().unwrap() > 0);
  assert!(first["threads"].as_u64().unwrap() >= 2);
  assert!(first["cpu_seconds"].as_f64().unwrap() >= 0.0);
  assert_eq!(
    first["identity"],
    request(&path, b"snapshot\n")["identity"]
  );
  signals.shutdown().request();
  assert_eq!(request(&path, b"snapshot\n")["state"], "stopping");
  drop(endpoint);
  assert!(!path.exists());
}

#[test]
fn test_open_refuses_active_socket_and_preserves_existing_files() {
  let file = File::open("/dev/null").unwrap();
  let signals = Signals::install().unwrap();
  let path = path("occupied");
  let first =
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).unwrap();
  assert!(
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).is_err()
  );
  assert_eq!(request(&path, b"snapshot\n")["state"], "ready");
  drop(first);
  fs::write(&path, b"preserve").unwrap();
  assert!(
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).is_err()
  );
  assert_eq!(fs::read(&path).unwrap(), b"preserve");
  fs::remove_file(path).unwrap();
}

#[test]
fn test_open_reclaims_only_stale_socket_and_drop_preserves_replacement()
{
  let file = File::open("/dev/null").unwrap();
  let signals = Signals::install().unwrap();
  let path = path("stale");
  drop(UnixListener::bind(&path).unwrap());
  let endpoint =
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).unwrap();
  assert_eq!(request(&path, b"snapshot\n")["state"], "ready");
  fs::remove_file(&path).unwrap();
  fs::write(&path, b"replacement").unwrap();
  drop(endpoint);
  assert_eq!(fs::read(&path).unwrap(), b"replacement");
  fs::remove_file(path).unwrap();
}

#[test]
fn test_requests_are_bounded_read_only_and_cannot_change_the_service() {
  let file = File::open("/dev/null").unwrap();
  let signals = Signals::install().unwrap();
  let path = path("bounded");
  let endpoint =
    Endpoint::open(&path, file.as_fd(), &signals.shutdown()).unwrap();
  for invalid in [b"stop\n".as_slice(), &[b'x'; 64]] {
    assert!(request(&path, invalid).get("error").is_some());
  }
  let idle = UnixStream::connect(&path).unwrap();
  std::thread::sleep(Duration::from_millis(300));
  assert_eq!(request(&path, b"snapshot\n")["state"], "ready");
  drop(idle);
  drop(endpoint);
}
