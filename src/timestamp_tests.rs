//! Timestamp source wire contract.
use crate::timestamp::Source;
#[test]
fn test_source_accepts_only_defined_v4l2_timestamp_origins() {
  assert_eq!(Source::try_from(0).unwrap(), Source::End);
  assert_eq!(Source::try_from(0x10000).unwrap(), Source::Start);
  assert!(Source::try_from(0x20000).is_err());
  assert!(Source::try_from(2).is_err());
}
