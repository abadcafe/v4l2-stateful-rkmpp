//! module: backend::flow_tests; responsibility: explicit/natural drain
//! matrix.
use super::flow::{Completion, End, Flow};
#[test]
fn test_complete_resume_preserves_explicit_references_and_resets_natural_sequence()
 {
  for end in [End::Explicit, End::Natural] {
    let mut flow = Flow::default();
    assert!(flow.running());
    flow.begin(end).unwrap();
    assert!(flow.waiting());
    assert!(flow.begin(end).is_err());
    assert_eq!(flow.complete().unwrap(), Completion::Notify);
    assert_eq!(flow.resume().unwrap(), end == End::Natural);
    assert!(flow.running());
    assert!(flow.complete().is_err());
  }
}
#[test]
fn test_abort_waits_for_inflight_drain_and_never_emits_eos() {
  for end in [End::Explicit, End::Natural] {
    let mut flow = Flow::default();
    flow.begin(end).unwrap();
    assert!(!flow.abort());
    assert!(!flow.abort());
    assert!(!flow.resume().unwrap());
    assert!(flow.waiting());
    assert_eq!(
      flow.complete().unwrap(),
      Completion::Continue { reset: end == End::Natural }
    );
    assert!(flow.running());
  }
}
#[test]
fn test_abort_completed_or_unsubmitted_drain_returns_to_running() {
  for end in [End::Explicit, End::Natural] {
    let mut flow = Flow::default();
    assert!(!flow.abort());
    flow.begin(end).unwrap();
    flow.complete().unwrap();
    assert_eq!(flow.abort(), end == End::Natural);
    assert!(flow.running());
  }
}
