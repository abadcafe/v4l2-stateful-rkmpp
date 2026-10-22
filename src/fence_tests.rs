//! module: fence_tests; responsibility: define sync-file completion.
use std::os::fd::AsFd;

use crate::fence::Fence;
use crate::notify::Notify;
#[test]
fn test_ready_preserves_completed_dma_fence_and_shared_descriptor_lifetime()
 {
  let buffer = dma_buf::memory::Buffer::allocate(4096).unwrap();
  let fence =
    Fence::new(Some(dma_buf::export_read(buffer.as_fd()).unwrap()));
  let lease = fence.lease().unwrap();
  fence.wait().unwrap();
  assert!(fence.ready().unwrap());
  assert!(fence.ready().unwrap());
  drop(fence);
  assert!(lease.as_fd().try_clone_to_owned().is_ok());
}
#[test]
fn test_ready_rejects_readability_without_sync_file_completion_status()
{
  let notify = Notify::new().unwrap();
  let fence =
    Fence::new(Some(notify.as_fd().try_clone_to_owned().unwrap()));
  assert!(!fence.ready().unwrap());
  notify.signal().unwrap();
  assert!(fence.ready().is_err());
  assert_eq!(
    fence.wait().unwrap_err().raw_os_error(),
    Some(libc::ENOTTY)
  );
}
#[test]
fn test_ready_completed_export_needs_no_descriptor() {
  let fence = Fence::new(None);
  fence.wait().unwrap();
  assert!(fence.ready().unwrap());
  assert!(fence.lease().is_none());
}
