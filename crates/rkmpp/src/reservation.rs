//! module: rkmpp::reservation; responsibility: retain exclusive decoder
//! buffer ownership during a GPU copy and subsequent display.
//! operations: retain, imported, index; entities: Reservation. tests:
//! reservation_tests, service lifecycle tests.
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

use super::ffi::{
  mpp_buffer_get_index_with_caller, mpp_buffer_inc_ref_with_caller,
  mpp_buffer_put_with_caller,
};
pub struct Reservation(NonNull<c_void>);
impl Reservation {
  /// SAFETY: the caller owns the live group containing this buffer.
  pub(super) unsafe fn retain(
    buffer: NonNull<c_void>,
  ) -> io::Result<Self> {
    let result = unsafe {
      mpp_buffer_inc_ref_with_caller(
        buffer.as_ptr(),
        c"capture_pin".as_ptr(),
      )
    };
    if result != 0 {
      return Err(io::Error::other("MPP buffer reference failed"));
    }
    Ok(Self(buffer))
  }

  /// SAFETY: the caller transfers one successful import reference.
  pub(super) unsafe fn imported(buffer: NonNull<c_void>) -> Self {
    Self(buffer)
  }

  pub fn index(&self) -> io::Result<usize> {
    let index = unsafe {
      mpp_buffer_get_index_with_caller(
        self.0.as_ptr(),
        c"capture_reserve".as_ptr(),
      )
    };
    usize::try_from(index).map_err(|_| {
      io::Error::other("MPP returned an invalid reserved index")
    })
  }
}
impl Drop for Reservation {
  fn drop(&mut self) {
    unsafe {
      mpp_buffer_put_with_caller(
        self.0.as_ptr(),
        c"capture_release".as_ptr(),
      );
    }
  }
}
