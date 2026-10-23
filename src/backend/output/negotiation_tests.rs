//! module: output::negotiation_tests; responsibility: CAPTURE
//! transition matrix.
//! operations: test entrypoints; entities: none; tests:
//! test_announce_old_pool_cannot_acknowledge_new_layout,
//! test_stop_retains_pending_decoder_ack_until_new_pool_is_ready,
//! test_start_rejects_invalid_capacity_and_reuse_without_a_layout_change.
use super::negotiation::Negotiation;
#[test]
fn test_announce_old_pool_cannot_acknowledge_new_layout() {
  for acknowledge in [false, true] {
    let mut n = Negotiation::default();
    n.start(18, false).unwrap();
    n.commit(18);
    assert!(n.active());
    n.announce(true, acknowledge);
    assert!(!n.commit(18));
    assert!(n.waiting() && n.retiring());
    n.start(18, true).unwrap();
    assert_eq!(n.acknowledge(), acknowledge);
    assert!(n.commit(18));
    assert!(n.active() && !n.waiting());
  }
}
#[test]
fn test_stop_retains_pending_decoder_ack_until_new_pool_is_ready() {
  let mut n = Negotiation::default();
  n.announce(true, true);
  n.stop();
  assert!(n.paused());
  assert!(!n.commit(32));
  n.start(18, false).unwrap();
  assert!(!n.paused());
  assert!(!n.commit(17));
  assert!(n.acknowledge());
  assert!(n.commit(18));
}
#[test]
fn test_start_rejects_invalid_capacity_and_reuse_without_a_layout_change()
 {
  let mut n = Negotiation::default();
  for count in [0, 33, usize::MAX] {
    assert!(n.start(count, false).is_err());
  }
  assert!(n.start(18, true).is_err());
  assert!(!n.active());
  n.announce(false, false);
  n.start(32, false).unwrap();
  assert!(!n.commit(31));
  assert!(n.commit(32));
  assert!(n.start(32, true).is_err());
}
