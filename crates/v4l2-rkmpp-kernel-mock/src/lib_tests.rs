//! module: lib_tests; responsibility: specify native C ABI forwarding.
//! operations: tests; entities: none.
unsafe extern "C" {
  fn open64(
    pathname: *const libc::c_char,
    flags: libc::c_int,
    ...
  ) -> libc::c_int;
  fn ioctl(fd: libc::c_int, command: libc::c_ulong, ...)
  -> libc::c_int;
}

#[test]
fn test_open64_creation_preserves_mode_and_descriptor_flags() {
  use std::ffi::CString;
  use std::os::fd::{FromRawFd, OwnedFd};
  use std::os::unix::ffi::OsStrExt;
  use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

  let directory = std::env::temp_dir()
    .join(format!("rkmpp-open-{}", std::process::id()));
  std::fs::create_dir(&directory).unwrap();
  let path = directory.join("native");
  let name = CString::new(path.as_os_str().as_bytes()).unwrap();
  let flags =
    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;
  // SAFETY: the path is live and O_CREAT supplies the required mode.
  let fd = unsafe { open64(name.as_ptr(), flags, 0o640u32) };
  assert!(fd >= 0, "{}", std::io::Error::last_os_error());
  // SAFETY: open64 transferred this new descriptor exactly once.
  let owner = unsafe { OwnedFd::from_raw_fd(fd) };
  let reference = directory.join("reference");
  let _reference = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o640)
    .open(&reference)
    .unwrap();
  assert_eq!(
    std::fs::metadata(&path).unwrap().mode(),
    std::fs::metadata(reference).unwrap().mode()
  );
  assert_ne!(
    unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
    0
  );
  drop(owner);
  std::fs::remove_dir_all(directory).unwrap();
}

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
fn test_ioctl_native_pointer_receives_exact_pending_byte_count() {
  use std::io::Write;
  use std::os::fd::AsRawFd;

  let (input, mut output) =
    std::os::unix::net::UnixStream::pair().unwrap();
  output.write_all(b"payload").unwrap();
  let mut count = -1i32;
  // SAFETY: FIONREAD writes one int; input and count remain live.
  assert_eq!(
    unsafe {
      ioctl(
        input.as_raw_fd(),
        libc::FIONREAD as libc::c_ulong,
        &mut count as *mut i32,
      )
    },
    0
  );
  assert_eq!(count, 7);
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

#[test]
fn test_ioctl_argumentless_native_commands_preserve_descriptor_flags() {
  use std::os::fd::AsRawFd;
  let file = std::fs::File::open("/dev/null").unwrap();
  // SAFETY: FIONCLEX and FIOCLEX take no variadic argument. The fd is
  // retained throughout; F_GETFD also takes no additional argument.
  assert_eq!(
    unsafe { ioctl(file.as_raw_fd(), libc::FIONCLEX as libc::c_ulong) },
    0
  );
  assert_eq!(
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) }
      & libc::FD_CLOEXEC,
    0
  );
  assert_eq!(
    unsafe { ioctl(file.as_raw_fd(), libc::FIOCLEX as libc::c_ulong) },
    0
  );
  assert_ne!(
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) }
      & libc::FD_CLOEXEC,
    0
  );
}
