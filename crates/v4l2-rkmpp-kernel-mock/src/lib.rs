//! module: preload; responsibility: interpose the test process's
//! bridge system-call entrypoints. operations: ioctl, open64; entities:
//! none; tests: lib_tests, Python service process acceptance.
#[cfg(test)]
mod lib_tests;

/// Open a native file, redirecting only the selected test control path.
///
/// # Safety
/// pathname must address a C string and flags must have their required
/// optional mode argument, as specified by Linux open64.
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn open64(
  _pathname: *const libc::c_char,
  _flags: libc::c_int,
  _mode: usize,
) -> libc::c_int {
  // Preserve the optional mode register without constructing a Rust
  // VaList. The dispatcher uses it only for commands that supply it.
  #[cfg(target_arch = "aarch64")]
  core::arch::naked_asm!("b {}", sym dispatch_open);
  #[cfg(target_arch = "x86_64")]
  core::arch::naked_asm!("jmp {}", sym dispatch_open);
}

unsafe extern "C" fn dispatch_open(
  pathname: *const libc::c_char,
  flags: libc::c_int,
  mode: usize,
) -> libc::c_int {
  use std::ffi::{CStr, CString};
  use std::os::unix::ffi::OsStrExt;
  let mode = if flags & libc::O_CREAT != 0
    || flags & libc::O_TMPFILE == libc::O_TMPFILE
  {
    // Linux mode_t uses the low 32 bits of the variadic register.
    mode as libc::mode_t
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
#[unsafe(naked)]
pub unsafe extern "C" fn ioctl(
  _fd: libc::c_int,
  _command: libc::c_ulong,
  _argument: usize,
) -> libc::c_int {
  // Do not read a C variadic argument: many native ioctl commands have
  // none, including legacy commands without encoded direction/size.
  // Tail-forward the raw argument register. Every register bit pattern
  // is a valid usize; the kernel ignores it for argumentless commands.
  // The naked branch preserves all arguments and the caller's return
  // address without an intervening Rust prologue or register clobber.
  #[cfg(target_arch = "aarch64")]
  core::arch::naked_asm!("b {}", sym dispatch_ioctl);
  #[cfg(target_arch = "x86_64")]
  core::arch::naked_asm!("jmp {}", sym dispatch_ioctl);
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
compile_error!(
  "the kernel test interceptor requires an aarch64 or x86_64 ABI entry"
);

unsafe extern "C" fn dispatch_ioctl(
  fd: libc::c_int,
  command: libc::c_ulong,
  argument: usize,
) -> libc::c_int {
  use std::os::fd::IntoRawFd;
  let result =
    rpc::intercept(fd, command, argument).and_then(|result| {
      match result {
        Some(rpc::Completed::Accepted(owner)) => {
          Ok(Some(owner.into_raw_fd()))
        }
        Some(rpc::Completed::Response(response)) => {
          // SAFETY: only the native ioctl caller authorizes payload
          // writes. Transport and wire decoding above retain
          // all descriptor owners until this exact
          // publication boundary.
          unsafe { response.write(argument) }?;
          Ok(Some(0))
        }
        None => Ok(None),
      }
    });
  match result {
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
