//! module: rkmpp::reservation_tests; responsibility: specify exclusive
//! MPP output buffer leases through the published module interfaces.
//! operations: test entrypoints; entities: none; tests:
//! acquire/index/drop.
use dma_buf::Buffer;

use super::group::Group;

#[test]
fn test_index_retains_exclusive_lease_until_drop() {
  let mut group = Group::new(false).unwrap();
  group.commit(&[(7, Buffer::allocate(4096).unwrap())], 8).unwrap();
  let lease = group.reserve_capture(4096).unwrap().unwrap();
  assert_eq!(lease.index().unwrap(), 7);
  assert!(group.reserve_capture(4096).unwrap().is_none());
  drop(lease);
  assert_eq!(
    group.reserve_capture(4096).unwrap().unwrap().index().unwrap(),
    7
  );
}
