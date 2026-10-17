//! module: backend::capture_tests; responsibility: CAPTURE
//! write/display leases. Public operations only; MPP group allocation
//! is independently observable.
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
  pool.recycle(0, false).unwrap();
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert_eq!(target.index(), 0);
  assert_eq!(target.image().plane().stride(), 64);
  assert_eq!(target.commit(), 0);
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(0, false).unwrap();
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_some()
  );
  assert!(pool.recycle(1, false).is_err());
}
#[test]
fn test_prepare_write_abandoning_target_returns_ownership_without_qbuf()
{
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  pool.recycle(0, false).unwrap();
  drop(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap(),
  );
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_some()
  );
}
#[test]
fn test_prepare_write_retains_mpp_lease_for_error_or_gpu_output() {
  let mut decoder = decoder();
  let mut pool =
    CapturePool::new(1, vec![Buffer::allocate(4096).unwrap()]);
  pool.recycle(0, false).unwrap();
  decoder.import_capture(&pool.imports(), 1).unwrap();
  let target =
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().unwrap();
  assert!(decoder.reserve_capture(4096).unwrap().is_none());
  target.commit();
  assert!(decoder.reserve_capture(4096).unwrap().is_none());
  pool.recycle(0, false).unwrap();
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
  pool.recycle(1, false).unwrap();
  let imports = pool.imports();
  assert_eq!(imports.len(), 1);
  assert_eq!(imports[0].0, 1);
  decoder.import_capture(&imports, 2).unwrap();
  assert!(pool.imports().is_empty());
  pool
    .reuse(4096, decoder.reuse_output(false).unwrap().unwrap())
    .unwrap();
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(1, true).unwrap();
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_none()
  );
  pool.recycle(1, false).unwrap();
  assert!(pool.imports().is_empty());
  assert!(
    pool.prepare_write(&mut decoder, 4096, 64, 32).unwrap().is_some()
  );
  pool.forget_imports();
  assert_eq!(pool.imports().len(), 1);
  assert!(pool.reuse(4097, vec![]).is_err());
}
