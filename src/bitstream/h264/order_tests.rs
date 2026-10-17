//! module: bitstream::h264_order_tests; responsibility: specify non-IDR
//! picture-order resets. operations: tests; entities: none.
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
#[test]
fn test_non_idr_reference_marking_exposes_mmco5_boundary() {
  let mut order = Order::default();
  let sps = nal(0x67, "01000010000000000001111011011010000100001001");
  let pps = nal(0x68, "1100111000111100");
  assert!(order.observe(&sps).unwrap().is_none());
  assert!(order.observe(&pps).unwrap().is_none());
  assert!(
    !order.observe(&nal(0x61, "1110001000")).unwrap().unwrap().reset
  );
  assert!(
    order
      .observe(&nal(0x61, "1110010001001101"))
      .unwrap()
      .unwrap()
      .reset
  );
  // A disposable slice cannot contain reference picture marking.
  assert!(
    !order
      .observe(&nal(0x01, "1110001001001101"))
      .unwrap()
      .unwrap()
      .reset
  );
  assert!(order.observe(&nal(0x65, "1")).is_err());
}
#[test]
fn test_unknown_sets_and_truncated_marking_are_rejected() {
  let mut order = Order::default();
  assert!(order.observe(&nal(0x61, "1110001000")).is_err());
  assert!(order.observe(&[0x67]).is_err());
  assert!(order.observe(&[0x68]).is_err());
}

#[test]
fn test_idr_exposes_prior_output_suppression() {
  for discard in [false, true] {
    let mut order = Order::default();
    order
      .observe(&nal(
        0x67,
        "01000010000000000001111011011010000100001001",
      ))
      .unwrap();
    order.observe(&nal(0x68, "1100111000111100")).unwrap();
    let bits = format!("1011100001{}0", u8::from(discard));
    let marking = order.observe(&nal(0x65, &bits)).unwrap().unwrap();
    assert!(marking.idr);
    assert_eq!(marking.discard, discard);
    assert_eq!(marking.frame, 0);
  }
}

#[test]
fn test_observe_mmco5_field_pair_uses_normalized_reference_identity() {
  for bottom in [false, true] {
    let mut order = Order::default();
    // Main profile, POC type 2, max_num_ref_frames=4, interlaced
    // 128x128 picture, no frame-number gaps, crop or VUI.
    let sps = [
      "01001101", "00000000", "00011110", "1", "1", "011", "00101",
      "0", "0001000", "00100", "0", "0", "1", "0", "0",
    ]
    .concat();
    order.observe(&nal(0x67, &sps)).unwrap();
    order.observe(&nal(0x68, "1100111000111100")).unwrap();
    let idr = format!("1011100001{}100", u8::from(bottom));
    assert!(order.observe(&nal(0x65, &idr)).unwrap().unwrap().idr);
    let reset = format!("1011100011{}1001101", u8::from(bottom));
    let first = order.observe(&nal(0x61, &reset)).unwrap().unwrap();
    assert!(first.reset && !first.continuation);
    assert_eq!(first.frame, 1); // Raw syntax is retained for current-picture uses.
    let other = format!("1011100001{}0", u8::from(!bottom));
    let second = order.observe(&nal(0x61, &other)).unwrap().unwrap();
    assert!(second.continuation && !second.reset);
    assert_eq!(first.reference_id, second.reference_id);
    assert_eq!(second.references, vec![first.reference_id]);
  }
}
