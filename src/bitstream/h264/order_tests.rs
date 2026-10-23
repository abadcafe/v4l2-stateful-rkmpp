//! module: order_tests; responsibility: specify ordering from validated
//! H264 headers. operations: tests; entities: none.
//! tests: test_observe_non_idr_marking_exposes_mmco5_boundary,
//! test_observe_idr_exposes_prior_output_suppression,
//! test_observe_rejects_nonzero_idr_and_repeated_frame_identity,
//! test_observe_checks_canonical_difference_overflow_and_accepts_zero_encoding,
//! test_observe_mmco5_field_pair_uses_normalized_reference_identity.
use codec_parser::header::{H264, Marking, Operation};

use super::order::Order;
fn nal(header: u8, bits: &str) -> Vec<u8> {
  let mut out = vec![header];
  let mut padded = bits.to_owned();
  padded.push('1');
  while !padded.len().is_multiple_of(8) {
    padded.push('0');
  }
  out.extend(
    padded
      .as_bytes()
      .chunks(8)
      .map(|chunk| chunk.iter().fold(0, |v, b| (v << 1) | (b - b'0'))),
  );
  out
}
fn header(frame: u32, marking: Marking) -> H264 {
  H264 {
    sequence: std::rc::Rc::new(
      codec_parser::syntax::h264::parse(
        &nal(0x67, "01000010000000000001111011011010000100001001")[1..],
      )
      .unwrap(),
    ),
    frame,
    field: None,
    lsb: 0,
    bottom_delta: 0,
    delta: [0, 0],
    marking,
  }
}
#[test]
fn test_observe_non_idr_marking_exposes_mmco5_boundary() {
  let mut order = Order::default();
  assert!(!order.observe(&header(1, Marking::Sliding)).unwrap().reset);
  assert!(
    order
      .observe(&header(2, Marking::Adaptive(vec![Operation::Reset])))
      .unwrap()
      .reset
  );
  assert!(
    !order.observe(&header(1, Marking::Disposable)).unwrap().reset
  );
}
#[test]
fn test_observe_idr_exposes_prior_output_suppression() {
  for discard in [false, true] {
    let mut order = Order::default();
    let marking = order
      .observe(&header(0, Marking::Idr { discard, long: false }))
      .unwrap();
    assert!(marking.idr);
    assert_eq!(marking.discard, discard);
    assert_eq!(marking.frame, 0);
  }
}

#[test]
fn test_observe_rejects_nonzero_idr_and_repeated_frame_identity() {
  for marking in [
    header(1, Marking::Idr { discard: false, long: false }),
    header(0, Marking::Sliding),
    header(0, Marking::Disposable),
  ] {
    let mut order = Order::default();
    order
      .observe(&header(0, Marking::Idr { discard: false, long: false }))
      .unwrap();
    assert!(order.observe(&marking).is_err());
    order.reset();
    let resumed = order
      .observe(&header(0, Marking::Idr { discard: false, long: false }))
      .unwrap();
    assert!(resumed.idr && !resumed.continuation);
  }
}

#[test]
fn test_observe_checks_canonical_difference_overflow_and_accepts_zero_encoding()
 {
  for operation in [
    Operation::ForgetShort(u32::MAX),
    Operation::LongFromShort { difference: u32::MAX, index: 0 },
  ] {
    let mut order = Order::default();
    order
      .observe(&header(0, Marking::Idr { discard: false, long: false }))
      .unwrap();
    assert!(
      order
        .observe(&header(1, Marking::Adaptive(vec![operation])))
        .is_err()
    );
    order.reset();
    order
      .observe(&header(0, Marking::Idr { discard: false, long: false }))
      .unwrap();
    let next = order
      .observe(&header(
        1,
        Marking::Adaptive(vec![Operation::ForgetShort(0)]),
      ))
      .unwrap();
    assert_eq!(next.references, [next.reference_id]);
  }
}
#[test]
fn test_observe_mmco5_field_pair_uses_normalized_reference_identity() {
  for bottom in [false, true] {
    let mut order = Order::default();
    let sps = [
      "01001101", "00000000", "00011110", "1", "1", "011", "00101",
      "0", "0001000", "00100", "0", "0", "1", "0", "0",
    ]
    .concat();
    let sequence = std::rc::Rc::new(
      codec_parser::syntax::h264::parse(&nal(0x67, &sps)[1..]).unwrap(),
    );
    let mut idr =
      header(0, Marking::Idr { discard: false, long: false });
    idr.sequence = sequence.clone();
    idr.field = Some(bottom);
    assert!(order.observe(&idr).unwrap().idr);
    let mut reset =
      header(1, Marking::Adaptive(vec![Operation::Reset]));
    reset.sequence = sequence.clone();
    reset.field = Some(bottom);
    let first = order.observe(&reset).unwrap();
    assert!(first.reset && !first.continuation);
    assert_eq!(first.frame, 1);
    let mut other = header(0, Marking::Sliding);
    other.sequence = sequence;
    other.field = Some(!bottom);
    let second = order.observe(&other).unwrap();
    assert!(second.continuation && !second.reset);
    assert_eq!(first.reference_id, second.reference_id);
    assert_eq!(second.references, vec![first.reference_id]);
  }
}
