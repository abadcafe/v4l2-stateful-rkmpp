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
  let first = messages.next().unwrap().unwrap();
  assert_eq!((first.kind, first.payload), (128, vec![0x55]));
  let second = messages.next().unwrap().unwrap();
  assert_eq!((second.kind, second.payload), (383, vec![0x55; 256]));
  assert!(messages.next().unwrap().is_none());
  assert!(messages.next().unwrap().is_none());
}

#[test]
fn test_next_consumes_empty_payload_and_annex_b_trailing_zeros() {
  let mut messages = Messages::new(&[128, 0, 6, 0, 0x80, 0, 0]);
  for kind in [128, 6] {
    let message = messages.next().unwrap().unwrap();
    assert_eq!(message.kind, kind);
    assert!(message.payload.is_empty());
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
  assert_eq!(messages.next().unwrap().unwrap().payload, [0, 0, 3, 4]);
  let recovery = messages.next().unwrap().unwrap();
  assert_eq!((recovery.kind, recovery.payload), (6, vec![0x78]));
  assert!(messages.next().unwrap().is_none());
}
