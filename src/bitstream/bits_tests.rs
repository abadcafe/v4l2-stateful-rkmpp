//! module: bitstream::bits_tests; responsibility: specify bounded RBSP
//! reads. operations: tests; entities: none; tests: escapes and
//! truncation.
use super::bits::Bits;
#[test]
fn test_get_unescapes_and_rejects_truncation() {
  let mut bits = Bits::new(&[0, 0, 3, 1]);
  assert_eq!(bits.get(24).unwrap(), 1);
  assert!(bits.get(1).is_err());
}
#[test]
fn test_ue_rejects_overflow() {
  assert!(Bits::new(&[0; 8]).ue().is_err());
}

#[test]
fn test_from_rbsp_keeps_literal_emulation_prevention_value() {
  let mut bits = super::bits::Bits::from_rbsp(&[0, 0, 3, 4]);
  assert_eq!(bits.get(32).unwrap(), 0x00000304);
}

fn unsigned(value: u64) -> Vec<u8> {
  let code = value + 1;
  let width = 64 - code.leading_zeros() as usize;
  let mut bits = "0".repeat(width - 1) + &format!("{code:b}");
  while !bits.len().is_multiple_of(8) {
    bits.push('0');
  }
  bits
    .as_bytes()
    .chunks(8)
    .map(|chunk| {
      chunk.iter().fold(0u8, |n, &bit| (n << 1) | (bit - b'0'))
    })
    .collect()
}
#[test]
fn test_ue_covers_full_unsigned_domain_and_rejects_overflow() {
  for value in [
    0,
    1,
    (1u64 << 30) - 1,
    (1 << 31) - 1,
    (1 << 32) - 2,
    (1 << 32) - 1,
  ] {
    assert_eq!(
      Bits::from_rbsp(&unsigned(value)).ue().unwrap(),
      value as u32
    );
  }
  assert!(Bits::from_rbsp(&unsigned(1 << 32)).ue().is_err());
  for length in 0..8 {
    assert!(
      Bits::from_rbsp(&unsigned(u32::MAX as u64)[..length])
        .ue()
        .is_err()
    );
  }
}
#[test]
fn test_se_covers_signed_range_and_rejects_positive_overflow() {
  assert_eq!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64 - 2)).se().unwrap(),
    i32::MAX
  );
  assert_eq!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64 - 1)).se().unwrap(),
    -i32::MAX
  );
  assert!(Bits::from_rbsp(&unsigned(u32::MAX as u64)).se().is_err());
}

#[test]
fn test_ue_incremented_rejects_overflow_without_panicking() {
  assert_eq!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64 - 1))
      .ue_incremented()
      .unwrap(),
    u32::MAX
  );
  assert!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64))
      .ue_incremented()
      .is_err()
  );
}

#[test]
fn test_rbsp_distinguishes_payload_bits_from_trailing_marker() {
  for data in [vec![0x80], vec![0x80, 0, 0]] {
    let mut bits = Bits::from_rbsp(&data);
    assert!(!bits.more_rbsp_data());
    bits.finish_rbsp().unwrap();
    assert!(!bits.more_rbsp_data());
  }
  let mut bits = Bits::from_rbsp(&[0xA0]);
  assert!(bits.more_rbsp_data());
  assert_eq!(bits.get(2).unwrap(), 2);
  assert!(!bits.more_rbsp_data());
  bits.finish_rbsp().unwrap();
  for data in [vec![], vec![0], vec![0x81]] {
    assert!(Bits::from_rbsp(&data).finish_rbsp().is_err());
  }
}
