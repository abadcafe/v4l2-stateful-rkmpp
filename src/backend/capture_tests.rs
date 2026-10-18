//! module: backend::capture_tests; responsibility: CAPTURE
//! write/display leases. Public operations only; MPP group allocation
//! is independently observable.
use std::num::NonZeroU32;

use super::capture::CapturePool;
use crate::dma::Buffer;
use crate::mpp::Decoder;
use crate::notify::Notify;
fn decoder() -> Decoder {
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"H264"), Notify::new().unwrap())
      .unwrap();
  decoder.configure_output(false).unwrap();
  decoder
}
#[test]
fn test_prepare_write_requires_queue_and_commit_holds_until_recycle() {
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(7, vec![Buffer::allocate(4096).unwrap()]);
  assert_eq!(pool.registration(), (7, 1));
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(0, NonZeroU32::new(7).unwrap(), false).unwrap();
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert_eq!(target.plane().stride(), 64);
  assert_eq!(target.commit(), (0, NonZeroU32::new(7).unwrap()));
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(0, NonZeroU32::new(8).unwrap(), false).unwrap();
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert_eq!(target.commit(), (0, NonZeroU32::new(8).unwrap()));
  assert!(pool.recycle(1, NonZeroU32::new(8).unwrap(), false).is_err());
}
#[test]
fn test_prepare_write_abandoning_target_returns_ownership_without_qbuf()
{
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  pool.recycle(0, NonZeroU32::new(7).unwrap(), false).unwrap();
  drop(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap(),
  );
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert_eq!(target.commit(), (0, NonZeroU32::new(7).unwrap()));
}
#[test]
fn test_prepare_write_retains_mpp_lease_for_error_or_gpu_output() {
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  pool.recycle(0, NonZeroU32::new(7).unwrap(), false).unwrap();
  decoder.import_capture(&pool.imports(), 1).unwrap();
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert!(decoder.reserve_capture(4096).unwrap().is_none());
  target.commit();
  assert!(decoder.reserve_capture(4096).unwrap().is_none());
  pool.recycle(0, NonZeroU32::new(7).unwrap(), false).unwrap();
  assert!(decoder.reserve_capture(4096).unwrap().is_some());
}
#[test]
fn test_reuse_needs_requeue_and_preserves_registered_ownership() {
  let mut decoder = decoder();
  let mut pool = CapturePool::new(
    1,
    (0..2).map(|_| Buffer::allocate(4096).unwrap()).collect(),
  );
  assert!(pool.imports().is_empty());
  pool.recycle(1, NonZeroU32::new(8).unwrap(), false).unwrap();
  let imports = pool.imports();
  assert_eq!(imports.len(), 1);
  assert_eq!(imports[0].0, 1);
  decoder.import_capture(&imports, 2).unwrap();
  assert!(pool.imports().is_empty());
  pool.reuse(4096, decoder.reuse_output(false).unwrap()).unwrap();
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(1, NonZeroU32::new(9).unwrap(), true).unwrap();
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(1, NonZeroU32::new(8).unwrap(), false).unwrap();
  assert!(pool.imports().is_empty());
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_some()
  );
  pool.reuse(4096, None).unwrap();
  assert!(pool.imports().is_empty());
  pool.recycle(1, NonZeroU32::new(8).unwrap(), false).unwrap();
  assert_eq!(pool.imports().len(), 1);
  assert!(pool.reuse(4097, None).is_err());
}

#[test]
fn test_fits_requires_capacity_and_every_allocation() {
  let pool = CapturePool::new(
    1,
    vec![
      Buffer::allocate(4096).unwrap(),
      Buffer::allocate(8192).unwrap(),
    ],
  );
  assert!(pool.fits(2, 4096));
  assert!(!pool.fits(3, 4096));
  assert!(!pool.fits(2, 8192));
}

#[test]
fn test_recycle_duplicate_queue_preserves_original_submission() {
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  let original = NonZeroU32::new(7).unwrap();
  pool.recycle(0, original, false).unwrap();
  assert!(pool.recycle(0, NonZeroU32::new(8).unwrap(), false).is_err());
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert_eq!(target.commit(), (0, original));
}
