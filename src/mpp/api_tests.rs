//! module: mpp::api_tests; responsibility: specify foreign dispatch
//! table validation through the public import boundary.
use std::ffi::c_void;
use std::ptr::NonNull;

use super::api::Api;

// An independently constructed C ABI producer; no implementation fields
// of the validated Rust dispatch table are inspected by these tests.
#[repr(C)]
struct Producer {
  size: u32,
  version: u32,
  decode: Option<
    unsafe extern "C" fn(
      *mut c_void,
      *mut c_void,
      *mut *mut c_void,
    ) -> i32,
  >,
  put: usize,
  get:
    Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32>,
  other: [usize; 9],
  reset: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
  control:
    Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> i32>,
  reserved: [u32; 16],
}
unsafe extern "C" fn decode(
  _: *mut c_void,
  _: *mut c_void,
  _: *mut *mut c_void,
) -> i32 {
  11
}
unsafe extern "C" fn get(_: *mut c_void, _: *mut *mut c_void) -> i32 {
  12
}
unsafe extern "C" fn reset(_: *mut c_void) -> i32 {
  13
}
unsafe extern "C" fn control(
  _: *mut c_void,
  _: u32,
  _: *mut c_void,
) -> i32 {
  14
}
fn producer() -> Producer {
  Producer {
    size: std::mem::size_of::<Producer>() as u32,
    version: 0,
    decode: Some(decode),
    put: 0,
    get: Some(get),
    other: [0; 9],
    reset: Some(reset),
    control: Some(control),
    reserved: [0; 16],
  }
}

#[test]
fn test_read_rejects_short_allocation_before_borrowing_table() {
  let mut size = 4u32;
  // SAFETY: exactly the advertised four-byte prefix exists. Import
  // must reject it without forming a reference to the full C table.
  assert!(
    unsafe { Api::read(NonNull::from(&mut size).cast()) }.is_err()
  );
}

#[test]
fn test_read_requires_every_used_entry() {
  for missing in 0..4 {
    let mut record = producer();
    match missing {
      0 => record.decode = None,
      1 => record.get = None,
      2 => record.reset = None,
      _ => record.control = None,
    }
    // SAFETY: record owns the complete advertised initialized ABI.
    assert!(
      unsafe { Api::read(NonNull::from(&mut record).cast()) }.is_err()
    );
  }
}

#[test]
fn test_read_detaches_validated_entries_from_table_storage() {
  let table = {
    let mut record = producer();
    // SAFETY: all entries match their declared C signatures.
    unsafe { Api::read(NonNull::from(&mut record).cast()) }.unwrap()
  };
  let null = std::ptr::null_mut();
  // SAFETY: this test's functions accept null and never dereference it.
  assert_eq!(
    unsafe { (table.decode)(null, null, std::ptr::null_mut()) },
    11
  );
  assert_eq!(unsafe { (table.get)(null, std::ptr::null_mut()) }, 12);
  assert_eq!(unsafe { (table.reset)(null) }, 13);
  assert_eq!(unsafe { (table.control)(null, 0, null) }, 14);
}
