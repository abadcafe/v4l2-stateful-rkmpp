//! module: dma::access_tests; responsibility: specify CPU ownership,
//! transient synchronization retries and terminal release failures.
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};

use super::access::{Lease, Owner, State};

#[derive(Clone, Default)]
struct Storage {
  state: Arc<State>,
  responses: Arc<Mutex<VecDeque<i32>>>,
  calls: Arc<Mutex<Vec<(bool, bool)>>>,
}
impl Owner for Storage {
  fn state(&self) -> &State {
    &self.state
  }

  fn synchronize(&self, start: bool, write: bool) -> io::Result<()> {
    self.calls.lock().unwrap().push((start, write));
    match self.responses.lock().unwrap().pop_front().unwrap_or(0) {
      0 => Ok(()),
      errno => Err(io::Error::from_raw_os_error(errno)),
    }
  }
}
#[test]
fn test_new_excludes_aliases_until_finish_or_drop() {
  let storage = Storage::default();
  let lease = Lease::new(storage.clone(), false).unwrap();
  assert_eq!(
    Lease::new(storage.clone(), true).err().unwrap().kind(),
    io::ErrorKind::WouldBlock
  );
  lease.finish().unwrap();
  drop(Lease::new(storage.clone(), true).unwrap());
  Lease::new(storage.clone(), false).unwrap().finish().unwrap();
  assert_eq!(
    *storage.calls.lock().unwrap(),
    [
      (true, false),
      (false, false),
      (true, true),
      (false, true),
      (true, false),
      (false, false)
    ]
  );
}
#[test]
fn test_new_failed_start_releases_claim() {
  let storage = Storage::default();
  storage.responses.lock().unwrap().push_back(libc::EIO);
  assert_eq!(
    Lease::new(storage.clone(), false).err().unwrap().raw_os_error(),
    Some(libc::EIO)
  );
  Lease::new(storage, true).unwrap().finish().unwrap();
}
#[test]
fn test_finish_retries_transient_start_and_end_failures() {
  // DMA-BUF UAPI requires restarting SYNC for both EINTR and EAGAIN.
  let storage = Storage::default();
  storage.responses.lock().unwrap().extend([
    libc::EINTR,
    libc::EAGAIN,
    0,
    libc::EAGAIN,
    libc::EINTR,
    0,
  ]);
  Lease::new(storage.clone(), true).unwrap().finish().unwrap();
  assert_eq!(
    *storage.calls.lock().unwrap(),
    [
      (true, true),
      (true, true),
      (true, true),
      (false, true),
      (false, true),
      (false, true)
    ]
  );
  Lease::new(storage, false).unwrap().finish().unwrap();
}
#[test]
fn test_finish_terminal_failure_prevents_alias_reentry() {
  let storage = Storage::default();
  let lease = Lease::new(storage.clone(), true).unwrap();
  storage.responses.lock().unwrap().push_back(libc::EIO);
  assert_eq!(
    lease.finish().unwrap_err().raw_os_error(),
    Some(libc::EIO)
  );
  for write in [false, true] {
    assert_eq!(
      Lease::new(storage.clone(), write).err().unwrap().raw_os_error(),
      Some(libc::EIO)
    );
  }
  assert_eq!(
    *storage.calls.lock().unwrap(),
    [(true, true), (false, true)]
  );
}
#[test]
fn test_drop_terminal_failure_prevents_alias_reentry() {
  let storage = Storage::default();
  let lease = Lease::new(storage.clone(), false).unwrap();
  storage.responses.lock().unwrap().push_back(libc::EIO);
  drop(lease);
  assert_eq!(
    Lease::new(storage.clone(), true).err().unwrap().raw_os_error(),
    Some(libc::EIO)
  );
  assert_eq!(
    *storage.calls.lock().unwrap(),
    [(true, false), (false, false)]
  );
}
