//! Public HEVC SPS geometry contract.
#[test]
fn test_parse_rejects_truncated_sps() {
  for length in 0..20 {
    assert!(super::hevc::parse(&vec![0; length]).is_err());
  }
}

fn sequence(layers: &[(u32, u32, u32)], all: bool) -> Vec<u8> {
  sequence_with_chroma(layers, all, 1)
}

fn sequence_with_chroma(
  layers: &[(u32, u32, u32)],
  all: bool,
  chroma: u32,
) -> Vec<u8> {
  fn ue(value: u32) -> String {
    let code = format!("{:b}", u64::from(value) + 1);
    format!("{}{}", "0".repeat(code.len() - 1), code)
  }
  let mut bits =
    format!("0000{:03b}1{}", layers.len() - 1, "0".repeat(96));
  if layers.len() > 1 {
    bits.push_str(&"0".repeat(16));
  }
  for value in [0, chroma] {
    bits.push_str(&ue(value));
  }
  if chroma == 3 {
    bits.push('0');
  }
  for value in [64, 64] {
    bits.push_str(&ue(value));
  }
  bits.push('0');
  for _ in 0..3 {
    bits.push_str(&ue(0));
  }
  bits.push(if all { '1' } else { '0' });
  let selected = if all { layers } else { &layers[layers.len() - 1..] };
  for &(capacity, reorder, increase) in selected {
    for value in [capacity - 1, reorder, increase] {
      bits.push_str(&ue(value));
    }
  }
  for _ in 0..6 {
    bits.push('1');
  }
  bits.push_str("000010"); // no scaling, AMP, SAO, PCM; zero RPS sets, no long-term.
  bits.push('1');
  while !bits.len().is_multiple_of(8) {
    bits.push('0');
  }
  bits
    .as_bytes()
    .chunks(8)
    .map(|c| c.iter().fold(0, |v, b| v * 2 + (b - b'0')))
    .collect()
}

#[test]
fn test_parse_rejects_reserved_temporal_layer_and_chroma_values() {
  for layers in [1, 7] {
    for chroma in 0..=3 {
      assert!(
        super::hevc::parse(&sequence_with_chroma(
          &vec![(1, 0, 0); layers],
          true,
          chroma
        ))
        .is_ok()
      );
    }
  }
  assert!(
    super::hevc::parse(&sequence(&[(1, 0, 0); 8], true)).is_err()
  );
  for chroma in [4, 255, u32::MAX] {
    assert!(
      super::hevc::parse(&sequence_with_chroma(
        &[(1, 0, 0)],
        true,
        chroma
      ))
      .is_err()
    );
  }
}
#[test]
fn test_parse_preserves_each_temporal_layers_buffering_and_latency() {
  let s = super::hevc::parse(&sequence(&[(1, 0, 0), (4, 2, 3)], true))
    .unwrap();
  assert_eq!(s.layers.len(), 2);
  assert_eq!(s.layers[0].capacity(), 1);
  assert_eq!(s.layers[0].latency(), None);
  assert_eq!(s.layers[1].capacity(), 4);
  assert_eq!(s.layers[1].reorder(), 2);
  assert_eq!(s.layers[1].latency(), Some(4));
  assert_eq!(s.geometry.limits, s.layers[1]);
}
#[test]
fn test_parse_infers_lower_layer_limits_from_highest_when_not_signaled()
{
  let s = super::hevc::parse(&sequence(&[(1, 0, 0), (4, 2, 1)], false))
    .unwrap();
  assert_eq!(s.layers[0], s.layers[1]);
  assert_eq!(s.layers[0].latency(), Some(2));
}
#[test]
fn test_parse_rejects_reorder_above_hevc_buffering_minus_one() {
  assert!(super::hevc::parse(&sequence(&[(2, 2, 0)], true)).is_err());
}

#[test]
fn test_parse_accepts_full_legal_latency_domain_without_arithmetic_truncation()
 {
  for increase in [(1u32 << 31) - 1, u32::MAX - 1] {
    let s = super::hevc::parse(&sequence(&[(16, 15, increase)], true))
      .unwrap();
    assert_eq!(s.layers[0].latency(), Some(u64::from(increase) + 14));
  }
  assert!(
    super::hevc::parse(&sequence(&[(16, 15, u32::MAX)], true)).is_err()
  );
}
