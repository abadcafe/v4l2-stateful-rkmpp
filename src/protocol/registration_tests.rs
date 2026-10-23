//! module: registration_tests; responsibility: registered resource wire
//! layout.
//! operations: test entrypoints; entities: none; tests:
//! test_request_initializes_metadata_and_no_owned_file_descriptors.
use super::registration::{Descriptor, MAX_BUFFERS, Pool, request};
#[test]
fn test_request_initializes_metadata_and_no_owned_file_descriptors() {
  assert_eq!(std::mem::size_of::<Pool>(), 288);
  assert_eq!(std::mem::size_of::<Descriptor>(), 8);
  assert_eq!(std::mem::offset_of!(Pool, buffers), 32);
  assert_eq!(MAX_BUFFERS, 32);
  let pool = request(10, 7, 13);
  assert_eq!(
    (
      pool.version,
      pool.kind,
      pool.epoch,
      pool.pool_id,
      pool.count,
      pool.reserved
    ),
    (crate::protocol::VERSION, 10, 7, 13, 0, [0; 2])
  );
  assert!(
    pool.buffers.iter().all(|entry| entry.fd == -1 && entry.size == 0)
  );
}
