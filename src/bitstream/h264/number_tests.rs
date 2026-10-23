//! module: h264::number_tests; responsibility: define cyclic frame
//! numbers. operations: Rust test cases; entities: none; tests:
//! construction bounds, identity and bounded cyclic advancement.
use super::number::Number;

#[test]
fn test_new_rejects_unrepresentable_bits_and_frame_values() {
  for bits in [0, 3, 17, 32, usize::MAX] {
    assert!(Number::new(0, bits).is_err());
  }
  for bits in 4..=16 {
    let modulus = 1 << bits;
    assert!(Number::new(modulus, bits).is_err());
    assert!(Number::new(u32::MAX, bits).is_err());
    let last = Number::new(modulus - 1, bits).unwrap();
    assert_eq!(last.value(), modulus - 1);
    assert_eq!(last.modulus(), modulus);
    assert_eq!(last.advance(1), Number::new(0, bits).unwrap());
    assert_eq!(last.advance(u32::MAX).value(), modulus - 2);
  }
}

#[test]
fn test_numbers_from_different_frame_domains_are_distinct() {
  assert_ne!(Number::new(0, 4).unwrap(), Number::new(0, 5).unwrap());
  assert_eq!(Number::default(), Number::new(0, 4).unwrap());
}
