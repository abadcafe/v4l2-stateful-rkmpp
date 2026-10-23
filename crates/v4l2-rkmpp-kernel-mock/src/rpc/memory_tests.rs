//! module: rpc::memory_tests; responsibility: define kernel-checked
//! user-address copies through the memory module's public interface.
//! operations: test entrypoints; entities: Mapping; tests:
//! test_read_and_write_copy_exact_live_extent,
//! test_read_and_write_invalid_addresses_return_efault,
//! test_write_readonly_mapping_returns_efault_without_mutation,
//! test_read_and_write_partial_page_copy_returns_efault.
use super::memory::{read, write};

struct Mapping {
  address: std::ptr::NonNull<libc::c_void>,
  size: usize,
}
impl Drop for Mapping {
  fn drop(&mut self) {
    // SAFETY: this test owner retains the complete mmap allocation.
    assert_eq!(
      unsafe { libc::munmap(self.address.as_ptr(), self.size) },
      0
    );
  }
}

#[test]
fn test_read_and_write_copy_exact_live_extent() {
  let mut bytes = [1u8, 2, 3, 4];
  let address = bytes.as_mut_ptr() as usize;
  assert_eq!(read(address, 4).unwrap(), bytes);
  // SAFETY: bytes is exclusively owned and writable for this extent.
  unsafe { write(address, &[5, 6, 7, 8]) }.unwrap();
  assert_eq!(bytes, [5, 6, 7, 8]);
}

#[test]
fn test_read_and_write_invalid_addresses_return_efault() {
  for address in [0, 1, usize::MAX] {
    assert_eq!(
      read(address, 4).unwrap_err().raw_os_error(),
      Some(libc::EFAULT)
    );
    // SAFETY: inaccessible addresses have no live Rust storage to
    // alias.
    assert_eq!(
      unsafe { write(address, &[0; 4]) }.unwrap_err().raw_os_error(),
      Some(libc::EFAULT)
    );
  }
}

#[test]
fn test_write_readonly_mapping_returns_efault_without_mutation() {
  let bytes = b"readonly";
  assert_eq!(
    read(bytes.as_ptr() as usize, bytes.len()).unwrap(),
    bytes
  );
  // SAFETY: the read-only mapping cannot be mutated by this syscall.
  assert_eq!(
    unsafe { write(bytes.as_ptr() as usize, b"modified") }
      .unwrap_err()
      .raw_os_error(),
    Some(libc::EFAULT)
  );
  assert_eq!(bytes, b"readonly");
}

#[test]
fn test_read_and_write_partial_page_copy_returns_efault() {
  // SAFETY: sysconf has no pointer arguments; mmap creates two fresh,
  // writable pages and MAP_FAILED is checked before ownership is taken.
  let page =
    usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
      .unwrap();
  let raw = unsafe {
    libc::mmap(
      std::ptr::null_mut(),
      page * 2,
      libc::PROT_READ | libc::PROT_WRITE,
      libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
      -1,
      0,
    )
  };
  assert_ne!(raw, libc::MAP_FAILED);
  let mapping = Mapping {
    address: std::ptr::NonNull::new(raw).unwrap(),
    size: page * 2,
  };
  let protected = mapping.address.as_ptr() as usize + page;
  // SAFETY: the page-aligned second half belongs to this mapping. No
  // Rust reference to either page is constructed while testing copies.
  assert_eq!(
    unsafe {
      libc::mprotect(
        protected as *mut libc::c_void,
        page,
        libc::PROT_NONE,
      )
    },
    0
  );
  assert_eq!(
    read(protected - 2, 4).unwrap_err().raw_os_error(),
    Some(libc::EFAULT)
  );
  // SAFETY: the accessible prefix is exclusively owned; the kernel
  // must reject the inaccessible suffix even after a partial transfer.
  assert_eq!(
    unsafe { write(protected - 2, &[1, 2, 3, 4]) }
      .unwrap_err()
      .raw_os_error(),
    Some(libc::EFAULT)
  );
}
