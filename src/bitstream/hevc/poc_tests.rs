//! module: hevc::poc_tests; responsibility: define H.265 picture order.
//! operations: test entrypoints; entities: none; tests: prevTid0
//! selection, wrap, random access and parameter bounds via Counter.
use super::poc::{Counter, Picture};
fn picture(lsb: u32) -> Picture {
  Picture {
    lsb,
    bits: 4,
    kind: 1,
    temporal_id: 0,
    random_access: false,
  }
}
#[test]
fn test_derive_wrap_and_temporal_reference_selection() {
  let mut counter = Counter::default();
  assert_eq!(counter.derive(picture(7)).unwrap(), 7);
  assert_eq!(counter.derive(picture(14)).unwrap(), 14);
  let mut upper = picture(5);
  upper.temporal_id = 1;
  assert_eq!(counter.derive(upper).unwrap(), 21);
  assert_eq!(counter.derive(picture(1)).unwrap(), 17);
  let mut random = picture(2);
  random.random_access = true;
  random.kind = 21;
  assert_eq!(counter.derive(random).unwrap(), 2);
}
#[test]
fn test_derive_leading_and_nonreference_do_not_change_previous_tid0() {
  for kind in [0, 2, 4, 6, 7, 8, 9] {
    let mut counter = Counter::default();
    counter.derive(picture(7)).unwrap();
    let mut excluded = picture(14);
    excluded.kind = kind;
    assert_eq!(counter.derive(excluded).unwrap(), 14);
    assert_eq!(counter.derive(picture(1)).unwrap(), 1);
  }
}
#[test]
fn test_derive_rejects_invalid_header_fields() {
  for bad in [
    Picture { bits: 17, ..picture(1) },
    Picture { lsb: 16, ..picture(1) },
    Picture { temporal_id: 7, ..picture(1) },
  ] {
    assert!(Counter::default().derive(bad).is_err());
  }
}
