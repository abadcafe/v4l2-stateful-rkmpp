//! module: service::supervisor_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_join_propagates_owned_worker_programming_panic
//! test_join_empty_supervisor_completes
//! test_join_waits_for_all_owned_workers
//! test_reap_preserves_running_workers_until_join
//! test_reap_completed_workers_allows_next_session
use super::supervisor::Supervisor;
use crate::shutdown::Signals;

#[test]
fn test_join_propagates_owned_worker_programming_panic() {
  let signals = Signals::install().unwrap();
  let mut supervisor = Supervisor::new(signals.shutdown()).unwrap();
  supervisor.start(|| panic!("worker programming error")).unwrap();
  assert!(
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      supervisor.join()
    }))
    .is_err()
  );
}
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

#[test]
fn test_join_empty_supervisor_completes() {
  let signals = Signals::install().unwrap();
  Supervisor::new(signals.shutdown()).unwrap().join();
}
#[test]
fn test_join_waits_for_all_owned_workers() {
  let completed = Arc::new(AtomicUsize::new(0));
  let signals = Signals::install().unwrap();
  let mut supervisor = Supervisor::new(signals.shutdown()).unwrap();
  for _ in 0..4 {
    let completed = completed.clone();
    supervisor
      .start(move || {
        completed.fetch_add(1, Ordering::Release);
      })
      .unwrap();
  }
  supervisor.join();
  assert_eq!(completed.load(Ordering::Acquire), 4);
}
#[test]
fn test_reap_preserves_running_workers_until_join() {
  let completed = Arc::new(AtomicUsize::new(0));
  let barrier = Arc::new(Barrier::new(2));
  let signals = Signals::install().unwrap();
  let mut supervisor = Supervisor::new(signals.shutdown()).unwrap();
  let signal = barrier.clone();
  let result = completed.clone();
  supervisor
    .start(move || {
      signal.wait();
      result.fetch_add(1, Ordering::Release);
    })
    .unwrap();
  supervisor.reap();
  assert_eq!(completed.load(Ordering::Acquire), 0);
  barrier.wait();
  supervisor.join();
  assert_eq!(completed.load(Ordering::Acquire), 1);
}
#[test]
fn test_reap_completed_workers_allows_next_session() {
  let completed = Arc::new(AtomicUsize::new(0));
  let signals = Signals::install().unwrap();
  let mut supervisor = Supervisor::new(signals.shutdown()).unwrap();
  for target in 1..4 {
    let counter = completed.clone();
    supervisor
      .start(move || {
        counter.fetch_add(1, Ordering::Release);
      })
      .unwrap();
    let deadline =
      std::time::Instant::now() + std::time::Duration::from_secs(2);
    while completed.load(Ordering::Acquire) < target {
      assert!(std::time::Instant::now() < deadline);
      std::thread::yield_now();
    }
    supervisor.reap();
  }
  supervisor.join();
  assert_eq!(completed.load(Ordering::Acquire), 3);
}

#[test]
fn test_reap_panic_notifies_without_new_session_and_teardown_joins_survivor()
 {
  let signals = Signals::install().unwrap();
  let mut supervisor = Supervisor::new(signals.shutdown()).unwrap();
  let wake = supervisor.notification();
  let finished = Arc::new(AtomicUsize::new(0));
  let done = finished.clone();
  let stop = signals.shutdown();
  supervisor
    .start(move || {
      while !stop.stopping() {
        std::thread::sleep(std::time::Duration::from_millis(1));
      }
      done.store(1, Ordering::Release);
    })
    .unwrap();
  supervisor.start(|| panic!("worker sentinel")).unwrap();
  assert!(wake.wait(std::time::Duration::from_secs(5)).unwrap());
  assert!(
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
      supervisor.reap();
    }))
    .is_err()
  );
  assert_eq!(finished.load(Ordering::Acquire), 1);
}
