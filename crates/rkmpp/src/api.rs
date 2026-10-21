//! module: rkmpp::api; responsibility: validate the foreign MPP
//! dispatch ABI before exposing callable entries. operations: read,
//! decode, get, reset, control; entities: Api, Raw, Handle; tests:
//! api_tests.
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

type Handle = *mut c_void;
#[repr(C)]
struct Raw {
  size: u32,
  version: u32,
  decode:
    Option<unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32>,
  put: Option<unsafe extern "C" fn(Handle, Handle) -> i32>,
  get: Option<unsafe extern "C" fn(Handle, *mut Handle) -> i32>,
  other: [usize; 9],
  reset: Option<unsafe extern "C" fn(Handle) -> i32>,
  control: Option<unsafe extern "C" fn(Handle, u32, Handle) -> i32>,
  reserved: [u32; 16],
}
// Entries remain unsafe: the decoder owner must supply live SDK handles
// and command-specific parameter storage. Optionality and table
// lifetime are resolved once at this boundary, not at every hardware
// operation.
pub(super) struct Api {
  pub decode: unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32,
  pub get: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
  pub reset: unsafe extern "C" fn(Handle) -> i32,
  pub control: unsafe extern "C" fn(Handle, u32, Handle) -> i32,
}
impl Api {
  /// # Safety
  /// Pointer addresses an aligned initialized size prefix, followed by
  /// the advertised readable MppApi bytes when large enough. Populated
  /// entries have the published C signatures and remain callable for
  /// the returned dispatch table's lifetime.
  pub(super) unsafe fn read(
    pointer: NonNull<c_void>,
  ) -> io::Result<Self> {
    // SAFETY: only the mandatory four-byte prefix is accessed here.
    let size =
      unsafe { pointer.cast::<u32>().as_ptr().read() } as usize;
    if size < std::mem::size_of::<Raw>() {
      return Err(incompatible());
    }
    // SAFETY: the caller's advertised storage now covers the full ABI.
    let raw = unsafe { pointer.cast::<Raw>().as_ref() };
    Ok(Self {
      decode: raw.decode.ok_or_else(incompatible)?,
      get: raw.get.ok_or_else(incompatible)?,
      reset: raw.reset.ok_or_else(incompatible)?,
      control: raw.control.ok_or_else(incompatible)?,
    })
  }
}
fn incompatible() -> io::Error {
  io::Error::other("incompatible MPP API table")
}
