//! module: backend::output_tests; responsibility: CAPTURE transaction
//! public contract.
use super::output::{Output, Status};
use crate::dma::Buffer;
use crate::notify::Notify;
#[test]
fn test_register_duplicate_pool_and_unknown_recycle_do_not_change_ownership()
 {
  let mut output = Output::new(Notify::new().unwrap());
  assert_eq!(output.id(), 0);
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  assert!(
    output
      .register(2, vec![Buffer::allocate(4096).unwrap()], None)
      .is_err()
  );
  assert_eq!(output.id(), 1);
  assert!(output.recycle(2, 0, None).is_err());
  assert!(output.recycle(1, 1, None).is_err());
  output.start(1, false, None).unwrap();
  output.recycle(1, 0, None).unwrap();
  assert_eq!(output.status(), Status::Active);
}
#[test]
fn test_stop_requires_explicit_start_before_replacement_can_run() {
  let mut output = Output::new(Notify::new().unwrap());
  output.stop(None).unwrap();
  assert_eq!(output.status(), Status::Stopped);
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  assert_eq!(output.status(), Status::Stopped);
  output.start(1, false, None).unwrap();
  assert_ne!(output.status(), Status::Stopped);
  assert!(output.start(1, true, None).is_err());
}
