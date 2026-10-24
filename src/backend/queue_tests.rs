//! module: backend::queue_tests; responsibility: define ordered input
//! and termination boundaries. operations: tests; entities: none;
//! tests: coincident/distinct boundaries, cancellation and bounded
//! drains.
use std::num::NonZeroU32;

use super::queue::{Queue, Queued};
use crate::timestamp::Source;

fn token(value: u32) -> NonZeroU32 {
  NonZeroU32::new(value).unwrap()
}

#[test]
fn test_expand_coalesces_stop_at_the_natural_boundary() {
  let mut queue = Queue::default();
  queue.submit(0, 11, Source::End, token(1));
  queue.drain();
  assert!(matches!(queue.pop(), Some(Queued::Analyze { .. })));
  queue.expand(&mut Vec::new(), 0, token(1), true);
  assert!(matches!(
    queue.pop(),
    Some(Queued::Release { damaged: false, .. })
  ));
  assert!(
    matches!(queue.pop(), Some(Queued::Eos(Some(boundary))) if boundary == token(1))
  );
  assert!(queue.is_empty());
}

#[test]
fn test_drain_preserves_an_already_known_natural_boundary() {
  let mut queue = Queue::default();
  queue.expand(&mut Vec::new(), 0, token(2), true);
  for _ in 0..1024 {
    queue.drain();
  }
  assert!(matches!(queue.pop(), Some(Queued::Release { .. })));
  assert!(
    matches!(queue.pop(), Some(Queued::Eos(Some(boundary))) if boundary == token(2))
  );
  assert!(queue.is_empty());
}

#[test]
fn test_expand_preserves_stop_after_inputs_in_the_next_sequence() {
  let mut queue = Queue::default();
  queue.submit(0, 11, Source::End, token(1));
  queue.submit(1, 22, Source::End, token(2));
  queue.drain();
  assert!(matches!(
    queue.pop(),
    Some(Queued::Analyze { index: 0, .. })
  ));
  queue.expand(&mut Vec::new(), 0, token(1), true);
  assert!(matches!(
    queue.pop(),
    Some(Queued::Release { index: 0, .. })
  ));
  assert!(
    matches!(queue.pop(), Some(Queued::Eos(Some(boundary))) if boundary == token(1))
  );
  assert!(matches!(
    queue.pop(),
    Some(Queued::Analyze { index: 1, timestamp: 22, .. })
  ));
  queue.expand(&mut Vec::new(), 1, token(2), false);
  assert!(matches!(
    queue.pop(),
    Some(Queued::Release { index: 1, .. })
  ));
  assert!(matches!(queue.pop(), Some(Queued::Eos(None))));
  assert!(queue.is_empty());
}

#[test]
fn test_abort_drain_retains_later_input_and_its_boundary() {
  let mut queue = Queue::default();
  queue.drain();
  queue.submit(3, 33, Source::Start, token(4));
  queue.drain();
  queue.abort_drain();
  assert!(matches!(
    queue.pop(),
    Some(Queued::Analyze { index: 3, .. })
  ));
  assert!(matches!(queue.pop(), Some(Queued::Eos(None))));
  assert!(queue.is_empty());
}

#[test]
fn test_damage_releases_failed_input_before_later_work() {
  let mut queue = Queue::default();
  queue.submit(1, 22, Source::End, token(2));
  queue.damaged(0, token(1));
  assert!(matches!(
    queue.front(),
    Some(Queued::Release { index: 0, damaged: true, .. })
  ));
  queue.pop();
  assert!(matches!(
    queue.front(),
    Some(Queued::Analyze { index: 1, .. })
  ));
  queue.clear();
  assert!(queue.is_empty());
}
