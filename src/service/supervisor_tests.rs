//! module: service::supervisor_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_join_propagates_owned_worker_programming_panic
//! test_join_empty_supervisor_completes
//! test_join_waits_for_all_owned_workers
//! test_reap_preserves_running_workers_until_join
//! test_reap_completed_workers_allows_next_session
use super::supervisor::Supervisor;

#[test]
fn test_join_propagates_owned_worker_programming_panic() {
  let mut supervisor = Supervisor::default();
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
  Supervisor::default().join();
}
#[test]
fn test_join_waits_for_all_owned_workers() {
  let completed = Arc::new(AtomicUsize::new(0));
  let mut supervisor = Supervisor::default();
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
  let mut supervisor = Supervisor::default();
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
  let mut supervisor = Supervisor::default();
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
