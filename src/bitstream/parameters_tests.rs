//! module: parameters_tests; responsibility: parameter identity
//! replacement.
use super::parameters::Parameters;
#[test]
fn test_insert_replaces_only_matching_kind_and_identity() {
  let mut sets = Parameters::default();
  sets.insert(8, 0, &[8, 1]).unwrap();
  sets.insert(7, 0, &[7, 2]).unwrap();
  sets.insert(7, 1, &[7, 3]).unwrap();
  sets.insert(7, 0, &[7, 4]).unwrap();
  assert_eq!(
    sets.configuration(),
    vec![0, 0, 0, 1, 7, 4, 0, 0, 0, 1, 7, 3, 0, 0, 0, 1, 8, 1]
  );
}
#[test]
fn test_configuration_empty_registry_has_no_bytes() {
  assert!(Parameters::default().configuration().is_empty());
}

#[test]
fn test_insert_repeated_and_shorter_parameters_preserve_exact_configuration()
 {
  let mut sets = Parameters::default();
  for _ in 0..3 {
    sets.insert(7, 0, &[7, 1, 2, 3]).unwrap();
    assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 1, 2, 3]);
  }
  sets.insert(7, 0, &[7, 4]).unwrap();
  assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 4]);
}

#[test]
fn test_insert_bounds_total_configuration_before_mutation() {
  let mut sets = Parameters::default();
  let full = vec![7; crate::submission::MAX_PACKET_SIZE - 4];
  sets.insert(7, 0, &full).unwrap();
  assert!(sets.insert(8, 0, &[8]).is_err());
  assert!(sets.insert(7, 0, &full).is_ok());
  assert_eq!(
    sets.configuration().len(),
    crate::submission::MAX_PACKET_SIZE
  );
  sets.insert(7, 0, &[7]).unwrap();
  sets.insert(8, 0, &[8]).unwrap();
  assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 0, 0, 0, 1, 8]);
}
