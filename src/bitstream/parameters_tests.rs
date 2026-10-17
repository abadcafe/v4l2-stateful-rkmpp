//! module: parameters_tests; responsibility: parameter identity
//! replacement.
use super::parameters::Parameters;
#[test]
fn test_insert_replaces_only_matching_kind_and_identity() {
  let mut sets = Parameters::default();
  sets.insert(8, 0, &[8, 1]);
  sets.insert(7, 0, &[7, 2]);
  sets.insert(7, 1, &[7, 3]);
  sets.insert(7, 0, &[7, 4]);
  assert_eq!(
    sets.configuration(),
    vec![0, 0, 0, 1, 7, 4, 0, 0, 0, 1, 7, 3, 0, 0, 0, 1, 8, 1]
  );
}
#[test]
fn test_configuration_empty_registry_has_no_bytes() {
  assert!(Parameters::default().configuration().is_empty());
}
