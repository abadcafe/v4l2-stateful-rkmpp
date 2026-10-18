//! module: presentation_tests; responsibility: normative output
//! scheduling.
use super::presentation::Presentation;
use crate::picture::{Boundary, Id, Limits, Output, Part, Plan};
#[derive(Clone)]
struct Spec {
  id: u64,
  order: i32,
  display: bool,
  limits: Limits,
  references: Vec<u64>,
  reference: bool,
  before: Boundary,
}
impl Spec {
  fn build(&self) -> Plan {
    Plan::new(
      Id::new(self.id),
      Part::Frame,
      self.references.iter().copied().map(Id::new).collect(),
      self.limits,
      self.reference,
      Output {
        order: self.order,
        display: self.display,
        before: self.before,
        drain_after: false,
      },
    )
  }
}
fn plan(id: u64, order: i32, reorder: usize) -> Spec {
  Spec {
    id,
    order,
    display: true,
    limits: Limits::new(16, reorder, None).unwrap(),
    references: vec![],
    reference: false,
    before: Boundary::Continue,
  }
}
#[test]
fn test_picture_reorders_and_drain_preserves_continuation() {
  let mut q = Presentation::default();
  for (id, poc) in [(1, 0), (2, 6), (3, 2), (4, 4)] {
    q.picture(id, &plan(id, poc, 2).build());
  }
  assert_eq!(q.take(), Some(1));
  assert_eq!(q.take(), Some(3));
  q.drain();
  assert_eq!(q.take(), Some(4));
  assert_eq!(q.take(), Some(2));
  q.picture(5, &plan(5, 0, 0).build());
  assert_eq!(q.take(), Some(5));
}
#[test]
fn test_prepare_discards_prior_before_source_change_barrier() {
  let mut q = Presentation::default();
  q.picture(1, &plan(1, 2, 4).build());
  let mut p = plan(2, 0, 0);
  p.before = Boundary::Discard;
  q.prepare(&p.build());
  q.barrier(10);
  assert_eq!(q.take(), Some(10));
  assert_eq!(q.take(), None);
}
#[test]
fn test_picture_bumps_at_declared_capacity_not_hardware_maximum() {
  let mut q = Presentation::default();
  for id in 1..=4 {
    let mut p = plan(id, id as i32, 4);
    p.limits = Limits::new(4, 4, None).unwrap();
    p.references = vec![1, 2];
    q.picture(id, &p.build());
  }
  let mut next = plan(5, 5, 4);
  next.limits = Limits::new(4, 4, None).unwrap();
  next.reference = true;
  next.references = vec![1, 2, 5];
  assert!(q.prepare(&next.build()));
  assert_eq!(q.take(), Some(1));
  assert_eq!(q.take(), Some(2));
  assert_eq!(q.take(), Some(3));
  assert_eq!(q.take(), None);
}
#[test]
fn test_prepare_lower_nonreference_picture_uses_direct_output() {
  let mut q = Presentation::default();
  for id in 1..=2 {
    let mut p = plan(id, id as i32 + 2, 2);
    p.limits = Limits::new(2, 2, None).unwrap();
    q.picture(id, &p.build());
  }
  let mut p = plan(3, 0, 2);
  p.limits = Limits::new(2, 2, None).unwrap();
  assert!(!q.prepare(&p.build()));
  q.picture(3, &p.build());
  assert_eq!(q.take(), Some(3));
  assert_eq!(q.take(), None);
}
#[test]
fn test_latency_counts_only_later_decoded_lower_poc_output_pictures() {
  let mut q = Presentation::default();
  let mut a = plan(1, 10, 4);
  a.limits = Limits::new(8, 4, Some(1)).unwrap();
  q.picture(1, &a.build());
  let mut b = a.clone();
  b.id = 2;
  b.order = 12;
  q.picture(2, &b.build());
  assert_eq!(q.take(), None);
  let mut hidden = a.clone();
  hidden.id = 3;
  hidden.order = 0;
  hidden.display = false;
  q.picture(3, &hidden.build());
  assert_eq!(q.take(), None);
  let mut low = a.clone();
  low.id = 4;
  low.order = 2;
  q.picture(4, &low.build());
  assert_eq!(q.take(), Some(4));
  assert_eq!(q.take(), Some(1));
  assert_eq!(q.take(), Some(2));
}
#[test]
fn test_hidden_picture_updates_references_without_becoming_output() {
  let mut q = Presentation::default();
  let mut p = plan(1, 0, 0);
  p.display = false;
  p.references = vec![1];
  q.picture(1, &p.build());
  assert_eq!(q.take(), None);
  q.drain();
  assert_eq!(q.take(), None);
}

#[test]
fn test_prepare_applies_new_reorder_bound_before_decoding() {
  let mut q = Presentation::default();
  q.picture(1, &plan(1, 10, 4).build());
  q.picture(2, &plan(2, 2, 4).build());
  let next = plan(3, 12, 1);
  assert!(q.prepare(&next.build()));
  assert_eq!(q.take(), Some(2));
  assert_eq!(q.take(), None);
}

#[test]
fn test_prepare_applies_new_latency_bound_before_decoding() {
  let mut q = Presentation::default();
  q.picture(1, &plan(1, 10, 4).build());
  q.picture(2, &plan(2, 2, 4).build());
  let mut next = plan(3, 12, 4);
  next.limits = Limits::new(16, 4, Some(1)).unwrap();
  assert!(q.prepare(&next.build()));
  assert_eq!(q.take(), Some(2));
  assert_eq!(q.take(), Some(1));
  assert_eq!(q.take(), None);
}

#[test]
fn test_picture_duplicate_identity_counts_once_toward_capacity() {
  let mut q = Presentation::default();
  let mut p = plan(1, 0, 4);
  p.limits = Limits::new(4, 4, None).unwrap();
  p.references = vec![2, 3, 4];
  q.picture(10, &p.build());
  q.picture(11, &p.build());
  assert_eq!(q.take(), None);
  q.drain();
  assert_eq!(q.take(), Some(10));
  assert_eq!(q.take(), Some(11));
}
