//! module: bitstream::h264::recovery_tests; responsibility: recovery
//! output contract.
//! operations: test entrypoints; entities: none; tests:
//! test_recovery_waits_for_reference_frame_number_and_output_order,
//! test_recovery_zero_and_reset,
//! test_sei_parses_recovery_and_rejects_truncated_payload,
//! test_parse_reserved_sei_does_not_hide_recovery_point,
//! test_parse_validates_following_messages_and_trailing_bits.
use super::number::Number;
use super::recovery::{Point, Recovery};
fn number(value: u32) -> Number {
  Number::new(value, 4).unwrap()
}
#[test]
fn test_recovery_waits_for_reference_frame_number_and_output_order() {
  let mut gate = Recovery::default();
  gate.begin(Point { count: 2, broken: false }, number(14)).unwrap();
  assert!(!gate.picture(number(14), 4, true));
  assert!(!gate.picture(number(15), 6, true));
  assert!(!gate.picture(number(0), 10, false));
  assert!(gate.picture(number(0), 10, true));
  assert!(!gate.picture(number(15), 8, false));
  assert!(gate.picture(number(1), 12, true));
}
#[test]
fn test_recovery_zero_and_reset() {
  let mut gate = Recovery::default();
  assert!(gate.picture(number(0), 0, true));
  gate.begin(Point { count: 0, broken: true }, number(3)).unwrap();
  assert!(gate.picture(number(3), 6, true));
  assert!(!gate.picture(number(2), 4, false));
  gate = Recovery::default();
  assert!(gate.picture(number(0), 0, true));
}
#[test]
fn test_sei_parses_recovery_and_rejects_truncated_payload() {
  assert_eq!(
    Point::parse(&[6, 1, 0b0111_1000, 0x80]).unwrap(),
    Some(Point { count: 2, broken: true })
  );
  assert_eq!(Point::parse(&[5, 1, 0, 0x80]).unwrap(), None);
  assert!(Point::parse(&[6, 2, 0]).is_err());
}

#[test]
fn test_parse_reserved_sei_does_not_hide_recovery_point() {
  for kind in [0x80, 137, 254, 255, 383] {
    let mut bytes = Vec::new();
    let mut remaining = kind;
    while remaining >= 255 {
      bytes.push(255);
      remaining -= 255;
    }
    bytes.extend([remaining as u8, 1, 0x55, 6, 1, 0x78, 0x80]);
    assert_eq!(
      Point::parse(&bytes).unwrap(),
      Some(Point { count: 2, broken: true })
    );
  }
}

#[test]
fn test_parse_validates_following_messages_and_trailing_bits() {
  for bytes in [
    vec![6, 1, 0x78],
    vec![6, 1, 0x78, 128, 2, 0x55],
    vec![6, 1, 0x78, 255],
  ] {
    assert!(Point::parse(&bytes).is_err());
  }
  assert_eq!(
    Point::parse(&[6, 1, 0x78, 128, 1, 0x55, 0x80, 0]).unwrap(),
    Some(Point { count: 2, broken: true })
  );
}
