//! module: bitstream::sei_tests; responsibility: SEI framing contract.
//! operations: tests; entities: none; tests: bounds, termination,
//! extended headers and one-time EBSP unescaping.
use super::sei::Messages;

#[test]
fn test_next_reads_reserved_types_and_extended_payload_sizes() {
  let mut data = vec![128, 1, 0x55, 255, 128, 255, 1];
  data.extend([0x55; 256]);
  data.push(0x80);
  let mut messages = Messages::new(&data);
  let mut first = messages.next().unwrap().unwrap();
  assert_eq!(first.kind, 128);
  assert_eq!(first.payload.get(8).unwrap(), 0x55);
  assert!(first.payload.get(1).is_err());
  let mut second = messages.next().unwrap().unwrap();
  assert_eq!(second.kind, 383);
  for _ in 0..256 {
    assert_eq!(second.payload.get(8).unwrap(), 0x55);
  }
  assert!(second.payload.get(1).is_err());
  assert!(messages.next().unwrap().is_none());
  assert!(messages.next().unwrap().is_none());
}

#[test]
fn test_next_consumes_empty_payload_and_annex_b_trailing_zeros() {
  let mut messages = Messages::new(&[128, 0, 6, 0, 0x80, 0, 0]);
  for kind in [128, 6] {
    let mut message = messages.next().unwrap().unwrap();
    assert_eq!(message.kind, kind);
    assert!(message.payload.get(1).is_err());
  }
  assert!(messages.next().unwrap().is_none());
}

#[test]
fn test_next_rejects_truncated_headers_payloads_and_missing_trailer() {
  for bytes in [
    vec![],
    vec![255],
    vec![6],
    vec![6, 255],
    vec![6, 2, 0x55],
    vec![0],
    vec![128, 1],
  ] {
    assert!(Messages::new(&bytes).next().is_err());
  }
  let mut messages = Messages::new(&[6, 0]);
  assert!(messages.next().unwrap().is_some());
  assert!(messages.next().is_err());
  let mut overflow = vec![255; 16_843_010];
  overflow.push(1);
  assert!(Messages::new(&overflow).next().is_err());
}

#[test]
fn test_next_unescapes_payload_once_without_reading_into_following_message()
 {
  let mut messages =
    Messages::new(&[5, 4, 0, 0, 3, 3, 4, 6, 1, 0x78, 0x80]);
  let mut first = messages.next().unwrap().unwrap();
  let mut recovery = messages.next().unwrap().unwrap();
  assert_eq!(first.payload.get(32).unwrap(), 0x00000304);
  assert!(first.payload.get(1).is_err());
  assert_eq!(recovery.kind, 6);
  assert_eq!(recovery.payload.get(8).unwrap(), 0x78);
  assert!(recovery.payload.get(1).is_err());
  assert!(messages.next().unwrap().is_none());
}
