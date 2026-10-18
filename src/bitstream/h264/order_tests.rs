//! module: order_tests; responsibility: specify ordering from validated
//! H264 headers. operations: tests; entities: none.
use super::order::Order;
use crate::packet_syntax::header::{H264, Marking, Operation};
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
    pps: 0,
    frame,
    field: None,
    lsb: 0,
    bottom_delta: 0,
    delta: [0, 0],
    marking,
  }
}
fn configured() -> Order {
  let mut order = Order::default();
  order
    .configure(&nal(
      0x67,
      "01000010000000000001111011011010000100001001",
    ))
    .unwrap();
  order.configure(&nal(0x68, "1100111000111100")).unwrap();
  order
}
#[test]
fn test_observe_non_idr_marking_exposes_mmco5_boundary() {
  let mut order = configured();
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
fn test_observe_unknown_sets_and_configure_truncated_sets_are_rejected()
{
  let mut order = Order::default();
  assert!(order.observe(&header(0, Marking::Sliding)).is_err());
  assert!(order.configure(&[0x67]).is_err());
  assert!(order.configure(&[0x68]).is_err());
}
#[test]
fn test_observe_idr_exposes_prior_output_suppression() {
  for discard in [false, true] {
    let mut order = configured();
    let marking = order
      .observe(&header(0, Marking::Idr { discard, long: false }))
      .unwrap();
    assert!(marking.idr);
    assert_eq!(marking.discard, discard);
    assert_eq!(marking.frame, 0);
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
    order.configure(&nal(0x67, &sps)).unwrap();
    order.configure(&nal(0x68, "1100111000111100")).unwrap();
    let mut idr =
      header(0, Marking::Idr { discard: false, long: false });
    idr.field = Some(bottom);
    assert!(order.observe(&idr).unwrap().idr);
    let mut reset =
      header(1, Marking::Adaptive(vec![Operation::Reset]));
    reset.field = Some(bottom);
    let first = order.observe(&reset).unwrap();
    assert!(first.reset && !first.continuation);
    assert_eq!(first.frame, 1);
    let mut other = header(0, Marking::Sliding);
    other.field = Some(!bottom);
    let second = order.observe(&other).unwrap();
    assert!(second.continuation && !second.reset);
    assert_eq!(first.reference_id, second.reference_id);
    assert_eq!(second.references, vec![first.reference_id]);
  }
}
