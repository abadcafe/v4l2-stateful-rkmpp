//! module: mpp::reservation; responsibility: retain exclusive decoder
//! buffer ownership during a GPU copy and subsequent display.
//! operations: acquire, index; entities: Reservation. tests:
//! reservation_tests, service lifecycle tests.
use std::ffi::{c_char, c_void};
use std::io;
use std::ptr::NonNull;
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_buffer_inc_ref_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_get_with_tag(
    group: *mut c_void,
    buffer: *mut *mut c_void,
    size: usize,
    tag: *const c_char,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_put_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
  fn mpp_buffer_get_index_with_caller(
    buffer: *mut c_void,
    caller: *const c_char,
  ) -> i32;
}
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

  pub(super) fn acquire(
    group: *mut c_void,
    size: usize,
  ) -> io::Result<Option<Self>> {
    let mut buffer = std::ptr::null_mut();
    let result = unsafe {
      mpp_buffer_get_with_tag(
        group,
        &mut buffer,
        size,
        c"v4l2-stateful-rkmpp".as_ptr(),
        c"capture_reserve".as_ptr(),
      )
    };
    if !super::status::reservation(result)? {
      return Ok(None);
    }
    NonNull::new(buffer).map(Self).map(Some).ok_or_else(|| {
      io::Error::other("MPP returned a null reserved buffer")
    })
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
