//! Public H.264 8.2.1 picture order contract; no decoder implementation
//! access.
use super::poc::{Counter, Field, Parameters, Picture};
fn frame(number: u32, lsb: u32) -> Picture {
  Picture {
    frame: number,
    frame_bits: 4,
    lsb,
    delta: [0, 0],
    bottom_delta: 0,
    field: Field::Frame,
    reference: true,
    idr: number == 0,
    reset: false,
  }
}
#[test]
fn test_derive_lsb_wrap_disposable_and_idr() {
  let mut counter = Counter::default();
  let p = Parameters::Lsb(4);
  assert_eq!(counter.derive(&p, frame(0, 0)).unwrap(), 0);
  assert_eq!(counter.derive(&p, frame(1, 7)).unwrap(), 7);
  assert_eq!(counter.derive(&p, frame(2, 14)).unwrap(), 14);
  let mut disposable = frame(3, 4);
  disposable.reference = false;
  assert_eq!(counter.derive(&p, disposable).unwrap(), 20);
  assert_eq!(counter.derive(&p, frame(3, 1)).unwrap(), 17);
  assert_eq!(counter.derive(&p, frame(0, 0)).unwrap(), 0);
}
#[test]
fn test_derive_mmco5_preserves_current_order_and_resets_next() {
  let mut counter = Counter::default();
  let p = Parameters::Lsb(4);
  counter.derive(&p, frame(0, 0)).unwrap();
  let mut reset = frame(1, 6);
  reset.reset = true;
  reset.bottom_delta = -2;
  assert_eq!(counter.derive(&p, reset).unwrap(), 4);
  assert_eq!(counter.derive(&p, frame(2, 3)).unwrap(), 3);
}
#[test]
fn test_derive_delta_cycles_fields_and_nonreference() {
  let mut counter = Counter::default();
  let p = Parameters::Delta {
    nonreference: -1,
    bottom: 1,
    offsets: vec![2, 3],
  };
  assert_eq!(counter.derive(&p, frame(0, 0)).unwrap(), 0);
  assert_eq!(counter.derive(&p, frame(1, 0)).unwrap(), 2);
  assert_eq!(counter.derive(&p, frame(2, 0)).unwrap(), 5);
  let mut bottom = frame(3, 0);
  bottom.field = Field::Bottom;
  bottom.delta[0] = 2;
  assert_eq!(counter.derive(&p, bottom).unwrap(), 10);
  let mut disposable = frame(4, 0);
  disposable.reference = false;
  assert_eq!(counter.derive(&p, disposable).unwrap(), 6);
}
#[test]
fn test_derive_frame_number_wrap_and_reset() {
  let mut counter = Counter::default();
  let p = Parameters::FrameNumber;
  assert_eq!(counter.derive(&p, frame(0, 0)).unwrap(), 0);
  assert_eq!(counter.derive(&p, frame(15, 0)).unwrap(), 30);
  let mut wrapped = frame(0, 0);
  wrapped.idr = false;
  assert_eq!(counter.derive(&p, wrapped).unwrap(), 32);
  let mut reset = frame(1, 0);
  reset.reset = true;
  assert_eq!(counter.derive(&p, reset).unwrap(), 34);
  assert_eq!(counter.derive(&p, frame(2, 0)).unwrap(), 4);
}
#[test]
fn test_derive_rejects_invalid_width_and_overflow_without_committing() {
  let mut counter = Counter::default();
  assert!(counter.derive(&Parameters::Lsb(33), frame(0, 0)).is_err());
  let p = Parameters::Delta {
    nonreference: 0,
    bottom: 0,
    offsets: vec![i32::MAX],
  };
  assert!(counter.derive(&p, frame(2, 0)).is_err());
  assert_eq!(
    counter.derive(&Parameters::FrameNumber, frame(0, 0)).unwrap(),
    0
  );
}
