//! module: mpp::reservation_tests; responsibility: specify exclusive
//! MPP output buffer leases through the published module interfaces.
//! operations: test entrypoints; entities: none; tests:
//! acquire/index/drop.
use super::group::Group;
use super::reservation::Reservation;
use crate::dma::Buffer;

#[test]
fn test_acquire_excludes_leased_buffer_until_drop() {
  let mut group = Group::new(false).unwrap();
  group.commit(&[(7, Buffer::allocate(4096).unwrap())], 8).unwrap();
  let lease =
    Reservation::acquire(group.handle(), 4096).unwrap().unwrap();
  assert_eq!(lease.index().unwrap(), 7);
  assert!(
    Reservation::acquire(group.handle(), 4096).unwrap().is_none()
  );
  drop(lease);
  assert_eq!(
    Reservation::acquire(group.handle(), 4096)
      .unwrap()
      .unwrap()
      .index()
      .unwrap(),
    7
  );
}
