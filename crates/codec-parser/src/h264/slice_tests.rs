//! module: slice_tests; responsibility: specify H.264 slice address
//! admission. operations: tests; entities: none; tests: frame, field
//! and MBAFF bounds, including the first invalid address and the full
//! u32 domain.
#![forbid(unsafe_code)]
use super::super::ffi;
use super::slice::validate;

#[test]
fn test_validate_accepts_only_defined_slice_types() {
  let geometry = crate::syntax::geometry::Geometry {
    coded_width: 128,
    coded_height: 128,
    ..Default::default()
  };
  for kind in (0..=14).chain([u32::MAX]) {
    let slice =
      ffi::GstH264SliceHdr { type_: kind, ..Default::default() };
    assert_eq!(validate(&slice, geometry, false).is_ok(), kind <= 9);
  }
}

#[test]
fn test_validate_address_respects_frame_field_and_macroblock_pair_units()
 {
  for field in [false, true] {
    for adaptive in [false, true] {
      let geometry = crate::syntax::geometry::Geometry {
        coded_width: 128,
        coded_height: 128,
        ..Default::default()
      };
      let count = if field || adaptive { 32 } else { 64 };
      for first in [0, count - 1, count, count + 1, u32::MAX] {
        let slice = ffi::GstH264SliceHdr {
          first_mb_in_slice: first,
          field_pic_flag: u8::from(field),
          ..Default::default()
        };
        assert_eq!(
          validate(&slice, geometry, adaptive).is_ok(),
          first < count,
          "field={field}, adaptive={adaptive}, first={first}"
        );
      }
    }
  }
}
