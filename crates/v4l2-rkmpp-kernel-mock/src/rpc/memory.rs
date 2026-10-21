//! module: rpc::memory; responsibility: copy ioctl user memory through
//! kernel-validated addresses. operations: read, write; entities: none;
//! tests: memory_tests. Invalid pointers never become Rust references.
use std::io;

pub(super) fn read(address: usize, size: usize) -> io::Result<Vec<u8>> {
  let mut bytes = vec![0u8; size];
  let local =
    libc::iovec { iov_base: bytes.as_mut_ptr().cast(), iov_len: size };
  let remote = libc::iovec {
    iov_base: address as *mut libc::c_void,
    iov_len: size,
  };
  // SAFETY: local describes writable owned bytes. The kernel checks
  // remote for accessibility and returns EFAULT for invalid addresses.
  let copied = unsafe {
    libc::process_vm_readv(libc::getpid(), &local, 1, &remote, 1, 0)
  };
  complete(copied, size)?;
  Ok(bytes)
}

/// # Safety
/// Any writable Rust storage at address must permit exclusive byte
/// writes for this call. Inaccessible addresses are allowed: the kernel
/// rejects them without creating a reference or dereferencing them
/// here.
pub(super) unsafe fn write(
  address: usize,
  bytes: &[u8],
) -> io::Result<()> {
  let local = libc::iovec {
    iov_base: bytes.as_ptr().cast_mut().cast(),
    iov_len: bytes.len(),
  };
  let remote = libc::iovec {
    iov_base: address as *mut libc::c_void,
    iov_len: bytes.len(),
  };
  // SAFETY: local describes readable borrowed bytes; process_vm_writev
  // validates the remote address and its write permissions in the
  // kernel.
  let copied = unsafe {
    libc::process_vm_writev(libc::getpid(), &local, 1, &remote, 1, 0)
  };
  complete(copied, bytes.len())
}

fn complete(copied: isize, expected: usize) -> io::Result<()> {
  if copied < 0 {
    Err(io::Error::last_os_error())
  } else if copied as usize != expected {
    Err(io::Error::from_raw_os_error(libc::EFAULT))
  } else {
    Ok(())
  }
}
