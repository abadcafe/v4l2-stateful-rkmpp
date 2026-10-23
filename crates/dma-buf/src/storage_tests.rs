//! module: dma_buf::storage_tests; responsibility: specify shared
//! storage identity across imports and allocation retirement.
//! operations: test entrypoints; entities: none; tests:
//! test_state_shares_only_live_matching_storage,
//! test_state_preserves_live_aliases_across_registry_collection.
use std::sync::Arc;

use super::storage::state;

#[test]
fn test_state_shares_only_live_matching_storage() {
  let first = state((u64::MAX, 1)).unwrap();
  let alias = state((u64::MAX, 1)).unwrap();
  let other = state((u64::MAX, 2)).unwrap();
  assert!(Arc::ptr_eq(&first, &alias));
  assert!(!Arc::ptr_eq(&first, &other));
  let retired = Arc::downgrade(&first);
  drop(first);
  drop(alias);
  assert!(retired.upgrade().is_none());
  let replacement = state((u64::MAX, 1)).unwrap();
  assert!(retired.upgrade().is_none());
  assert!(!Arc::ptr_eq(&replacement, &other));
}

#[test]
fn test_state_preserves_live_aliases_across_registry_collection() {
  let first = state((u64::MAX - 1, 0)).unwrap();
  for index in 1..1024 {
    drop(state((u64::MAX - 1, index)).unwrap());
  }
  assert!(Arc::ptr_eq(&first, &state((u64::MAX - 1, 0)).unwrap()));
}
