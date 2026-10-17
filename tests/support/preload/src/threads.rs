//! module: threads; responsibility: inject native thread admission
//! failure. operations: pthread_create; entities: Start; tests:
//! service/test_admission.py. A test-owned marker rejects new threads
//! without altering running threads.
use std::ffi::c_void;
type Start = extern "C" fn(*mut c_void) -> *mut c_void;
/// Delegate pthread creation unless the test's failure marker exists.
///
/// # Safety
/// Arguments must satisfy the native pthread_create contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_create(
  thread: *mut libc::pthread_t,
  attr: *const libc::pthread_attr_t,
  start: Start,
  argument: *mut c_void,
) -> libc::c_int {
  unsafe {
    let marker = libc::getenv(c"RKMPP_TEST_THREAD_FAILURE".as_ptr());
    if !marker.is_null() && libc::access(marker, libc::F_OK) == 0 {
      return libc::EAGAIN;
    }
    let symbol =
      libc::dlsym(libc::RTLD_NEXT, c"pthread_create".as_ptr());
    if symbol.is_null() {
      return libc::ENOSYS;
    }
    let native: unsafe extern "C" fn(
      *mut libc::pthread_t,
      *const libc::pthread_attr_t,
      Start,
      *mut c_void,
    ) -> libc::c_int = std::mem::transmute(symbol);
    native(thread, attr, start, argument)
  }
}
