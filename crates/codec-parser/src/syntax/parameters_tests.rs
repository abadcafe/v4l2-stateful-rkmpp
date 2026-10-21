//! module: parameters_tests; responsibility: parameter identity
//! replacement.
use super::parameters::Parameters;
fn insert(
  sets: &mut Parameters,
  kind: u8,
  id: u32,
  nal: &[u8],
) -> std::io::Result<()> {
  sets.commit(&[(kind, id, nal)])
}
#[test]
fn test_commit_replaces_only_matching_kind_and_identity() {
  let mut sets = Parameters::default();
  insert(&mut sets, 8, 0, &[8, 1]).unwrap();
  insert(&mut sets, 7, 0, &[7, 2]).unwrap();
  insert(&mut sets, 7, 1, &[7, 3]).unwrap();
  insert(&mut sets, 7, 0, &[7, 4]).unwrap();
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
fn test_commit_repeated_and_shorter_parameters_preserve_exact_configuration()
 {
  let mut sets = Parameters::default();
  for _ in 0..3 {
    insert(&mut sets, 7, 0, &[7, 1, 2, 3]).unwrap();
    assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 1, 2, 3]);
  }
  insert(&mut sets, 7, 0, &[7, 4]).unwrap();
  assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 4]);
}

#[test]
fn test_commit_bounds_total_configuration_before_mutation() {
  let mut sets = Parameters::default();
  let full =
    vec![7; crate::syntax::parameters::MAX_CONFIGURATION_SIZE - 4];
  insert(&mut sets, 7, 0, &full).unwrap();
  assert!(insert(&mut sets, 8, 0, &[8]).is_err());
  assert!(insert(&mut sets, 7, 0, &full).is_ok());
  assert_eq!(
    sets.configuration().len(),
    crate::syntax::parameters::MAX_CONFIGURATION_SIZE
  );
  insert(&mut sets, 7, 0, &[7]).unwrap();
  insert(&mut sets, 8, 0, &[8]).unwrap();
  assert_eq!(sets.configuration(), [0, 0, 0, 1, 7, 0, 0, 0, 1, 8]);
}

#[test]
fn test_commit_rejected_batch_preserves_all_saved_parameters() {
  let mut sets = Parameters::default();
  sets.commit(&[(7, 0, &[7, 1]), (8, 0, &[8, 1])]).unwrap();
  let full = vec![0; crate::syntax::parameters::MAX_CONFIGURATION_SIZE];
  assert!(sets.commit(&[(7, 0, &[7, 2]), (8, 0, &full)]).is_err());
  assert_eq!(
    sets.configuration(),
    [0, 0, 0, 1, 7, 1, 0, 0, 0, 1, 8, 1]
  );
  sets.commit(&[(7, 0, &[7, 3]), (7, 0, &[7, 4])]).unwrap();
  assert_eq!(
    sets.configuration(),
    [0, 0, 0, 1, 7, 4, 0, 0, 0, 1, 8, 1]
  );
}
