//! module: task_tests; responsibility: specify observable owned thread
//! exit. operations: tests; entities: none; tests: normal and panic
//! publication.
use std::time::Duration;

use crate::notify::Notify;
use crate::task::Task;
#[test]
fn test_spawn_normal_exit_notifies_before_observation() {
  let wake = Notify::new().unwrap();
  let task = Task::spawn("normal", wake.clone(), || {}).unwrap();
  assert!(wake.wait(Duration::from_secs(5)).unwrap());
  assert!(task.finished());
  task.join().unwrap();
}
#[test]
fn test_spawn_panic_exit_notifies_and_preserves_payload() {
  let wake = Notify::new().unwrap();
  let task =
    Task::spawn("panic", wake.clone(), || panic!("sentinel")).unwrap();
  assert!(wake.wait(Duration::from_secs(5)).unwrap());
  assert!(task.finished());
  let payload = task.join().unwrap_err();
  assert_eq!(payload.downcast_ref::<&str>(), Some(&"sentinel"));
}
