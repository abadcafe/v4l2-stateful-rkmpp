//! module: syntax::bits_tests; responsibility: specify bounded RBSP
//! reads. operations: tests; entities: none; tests: escapes and
//! truncation.
use super::bits::Bits;
#[test]
fn test_take_bytes_preserves_escape_context_and_payload_boundary() {
  let mut bits = Bits::new(&[0, 0, 3, 1, 0x55]);
  assert_eq!(bits.get(16).unwrap(), 0);
  let mut payload = bits.take_bytes(1).unwrap();
  assert_eq!(payload.get(8).unwrap(), 1);
  assert!(payload.get(1).is_err());
  assert_eq!(bits.get(8).unwrap(), 0x55);
}

#[test]
fn test_take_bytes_rejects_unaligned_or_truncated_ranges_atomically() {
  let mut bits = Bits::new(&[0x55]);
  assert!(bits.take_bytes(2).is_err());
  assert!(bits.take_bytes(usize::MAX).is_err());
  assert!(bits.take_bytes(0).unwrap().get(1).is_err());
  assert_eq!(bits.get(4).unwrap(), 5);
  assert!(bits.take_bytes(1).is_err());
  assert_eq!(bits.get(4).unwrap(), 5);
}

#[test]
fn test_get_unescapes_and_rejects_truncation() {
  let mut bits = Bits::new(&[0, 0, 3, 1]);
  assert_eq!(bits.get(24).unwrap(), 1);
  assert!(bits.get(1).is_err());
}

#[test]
fn test_get_rejects_invalid_escape_without_consuming_input() {
  for suffix in [4, 0x80, 0xff] {
    let bytes = [0, 0, 3, suffix];
    let mut bits = Bits::new(&bytes);
    assert!(bits.get(24).is_err());
    assert_eq!(bits.get(16).unwrap(), 0);
    assert!(bits.get(8).is_err());
  }
}

#[test]
fn test_finish_rbsp_does_not_treat_invalid_escape_as_end_of_input() {
  assert!(Bits::new(&[0x80, 0, 0, 3, 4]).finish_rbsp().is_err());
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
    Bits::from_rbsp(&unsigned(1 << 32)).se().unwrap(),
    i32::MIN
  );
  assert_eq!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64 - 2)).se().unwrap(),
    i32::MAX
  );
  assert_eq!(
    Bits::from_rbsp(&unsigned(u32::MAX as u64 - 1)).se().unwrap(),
    -i32::MAX
  );
  assert!(Bits::from_rbsp(&unsigned(u32::MAX as u64)).se().is_err());
  assert!(Bits::from_rbsp(&unsigned((1 << 32) + 1)).se().is_err());
  assert!(Bits::from_rbsp(&unsigned((1 << 32) + 2)).se().is_err());
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

#[test]
fn test_finish_rbsp_accepts_terminal_cabac_prevention_byte() {
  let mut bits = Bits::new(&[0x80, 0, 0, 3]);
  assert!(!bits.more_rbsp_data());
  bits.finish_rbsp().unwrap();
  assert!(!bits.more_rbsp_data());
}

#[test]
fn test_get_crosses_escape_and_byte_boundaries_without_consuming_on_error()
 {
  let mut bits = Bits::new(&[0xa0, 0, 0, 3, 2, 0x80]);
  assert_eq!(bits.get(4).unwrap(), 10);
  assert_eq!(bits.get(28).unwrap(), 2);
  assert!(bits.get(9).is_err());
  assert_eq!(bits.get(8).unwrap(), 0x80);
  assert_eq!(bits.get(0).unwrap(), 0);
}

#[test]
fn test_skip_large_escaped_payload_preserves_trailing_bits() {
  let mut bytes = vec![0x55; 1024 * 1024];
  bytes.extend_from_slice(&[0, 0, 3, 1, 0x80]);
  let mut bits = Bits::new(&bytes);
  bits.skip(1024 * 1024 * 8).unwrap();
  assert_eq!(bits.get(24).unwrap(), 1);
  assert!(!bits.more_rbsp_data());
  bits.finish_rbsp().unwrap();
}

#[test]
fn test_skip_matches_bit_reads_across_cached_and_escaped_bytes() {
  let data = [0xa5, 0, 0, 3, 1, 0x63, 0x80];
  for escaped in [false, true] {
    for prefix in 0..8 {
      for count in 0..32 {
        let mut actual = if escaped {
          Bits::new(&data)
        } else {
          Bits::from_rbsp(&data)
        };
        let mut expected = actual.clone();
        actual.skip(prefix).unwrap();
        for _ in 0..prefix {
          expected.get(1).unwrap();
        }
        actual.skip(count).unwrap();
        for _ in 0..count {
          expected.get(1).unwrap();
        }
        assert_eq!(actual.get(8).unwrap(), expected.get(8).unwrap());
      }
    }
  }
}
