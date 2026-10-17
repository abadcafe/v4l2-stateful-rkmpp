//! module: registration_tests; responsibility: registered resource wire
//! layout.
use super::registration::{Descriptor, MAX_BUFFERS, Pool};
#[test]
fn test_pool_default_has_no_owned_file_descriptors() {
  assert_eq!(std::mem::size_of::<Pool>(), 288);
  assert_eq!(std::mem::size_of::<Descriptor>(), 8);
  assert_eq!(std::mem::offset_of!(Pool, buffers), 32);
  assert_eq!(MAX_BUFFERS, 32);
  let pool = Pool::default();
  assert_eq!(
    (
      pool.version,
      pool.kind,
      pool.epoch,
      pool.pool_id,
      pool.count,
      pool.reserved
    ),
    (0, 0, 0, 0, 0, [0; 2])
  );
  assert!(
    pool.buffers.iter().all(|entry| entry.fd == -1 && entry.size == 0)
  );
}
