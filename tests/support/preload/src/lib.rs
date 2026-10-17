//! module: preload; responsibility: interpose the test process's
//! bridge system-call entrypoints. operations: ioctl, open64; entities:
//! none; tests: lib_tests, Python service process acceptance.
#![feature(c_variadic)]
#[cfg(test)]
mod lib_tests;

/// Open a native file, redirecting only the selected test control path.
///
/// # Safety
/// pathname must address a C string and flags must have their required
/// optional mode argument, as specified by Linux open64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn open64(
  pathname: *const libc::c_char,
  flags: libc::c_int,
  mut arguments: ...
) -> libc::c_int {
  use std::ffi::{CStr, CString};
  use std::os::unix::ffi::OsStrExt;
  let mode = if flags & libc::O_CREAT != 0
    || flags & libc::O_TMPFILE == libc::O_TMPFILE
  {
    // SAFETY: These flags require the caller's mode argument.
    unsafe { arguments.next_arg::<libc::mode_t>() }
  } else {
    0
  };
  let redirected = if !pathname.is_null()
    // SAFETY: pathname is the caller's live C string for this call.
    && unsafe { CStr::from_ptr(pathname) }.to_bytes()
      == b"/dev/v4l2-rkmpp-control"
    && std::env::var_os("RKMPP_TEST_KERNEL_SOCKET").is_some()
  {
    std::env::var_os("RKMPP_TEST_KERNEL_CONTROL")
      .and_then(|value| CString::new(value.as_bytes()).ok())
  } else {
    None
  };
  let target =
    redirected.as_ref().map_or(pathname, |value| value.as_ptr());
  // SAFETY: target remains live until the native openat call returns.
  unsafe {
    libc::syscall(libc::SYS_openat, libc::AT_FDCWD, target, flags, mode)
      as i32
  }
}
mod rpc;
#[cfg(test)]
mod rpc_tests;

/// Execute one ioctl through the process's selected bridge endpoint.
///
/// # Safety
/// The caller must meet Linux ioctl's pointer and variadic argument
/// requirements for the supplied command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ioctl(
  fd: libc::c_int,
  command: libc::c_ulong,
  mut arguments: ...
) -> libc::c_int {
  let argument = if command == 0x5280 {
    0
  } else {
    // SAFETY: Every other ioctl used by this process supplies its
    // third argument according to Linux's native ioctl convention.
    unsafe { arguments.next_arg::<usize>() }
  };
  // SAFETY: The caller retains the command's payload for this call.
  match unsafe { rpc::intercept(fd, command, argument) } {
    Ok(Some(value)) => value,
    Ok(None) => {
      // SAFETY: Delegating to the Linux syscall preserves the native
      // fd/command/argument and bypasses this interposed symbol.
      unsafe {
        libc::syscall(libc::SYS_ioctl, fd, command, argument) as i32
      }
    }
    Err(error) => {
      // SAFETY: errno is the calling thread's libc error slot.
      unsafe {
        *libc::__errno_location() =
          error.raw_os_error().unwrap_or(libc::EPROTO);
      }
      -1
    }
  }
}

mod threads;
