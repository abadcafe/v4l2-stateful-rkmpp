//! module: lib_tests; responsibility: specify native ioctl forwarding.
//! operations: tests; entities: none.
use super::{ioctl, open64};

#[test]
fn test_open64_unrelated_missing_path_retains_native_errno() {
  // SAFETY: The C path is live; read-only open needs no mode argument.
  let value = unsafe {
    open64(
      c"/rkmpp-mock-nonexistent-directory/file".as_ptr(),
      libc::O_RDONLY,
    )
  };
  assert_eq!(value, -1);
  assert_eq!(
    std::io::Error::last_os_error().raw_os_error(),
    Some(libc::ENOENT)
  );
}

#[test]
fn test_ioctl_unrelated_descriptor_retains_native_errno() {
  // SAFETY: The invalid fd has no userspace payload requirements.
  let value = unsafe { ioctl(-1, 0x1234, 0usize) };
  assert_eq!(value, -1);
  assert_eq!(
    std::io::Error::last_os_error().raw_os_error(),
    Some(libc::EBADF)
  );
}

#[test]
fn test_ioctl_accept_without_payload_retains_native_errno() {
  // SAFETY: ACCEPT has no third argument or userspace payload.
  let value = unsafe { ioctl(-1, 0x5280) };
  assert_eq!(value, -1);
  assert_eq!(
    std::io::Error::last_os_error().raw_os_error(),
    Some(libc::EBADF)
  );
}
