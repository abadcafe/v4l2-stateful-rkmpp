//! module: slice_tests; responsibility: specify HEVC CTB address
//! admission. operations: tests; entities: none; tests: first,
//! independent and dependent segments, non-power-of-two pictures and
//! empty geometry.
#![forbid(unsafe_code)]
use super::super::ffi;
use super::slice::validate;

#[test]
fn test_validate_accepts_only_defined_slice_types() {
  let parameters = ffi::GstH265PPS {
    picwidthinctbsy: 1,
    picheightinctbsy: 1,
    ..Default::default()
  };
  for kind in [0, 1, 2, 3, 4, 63] {
    let slice = ffi::GstH265SliceHdr {
      type_: kind,
      first_slice_segment_in_pic_flag: 1,
      ..Default::default()
    };
    assert_eq!(validate(&slice, &parameters).is_ok(), kind <= 2);
  }
  // Dependent segments inherit slice_type rather than carrying it.
  let dependent = ffi::GstH265SliceHdr {
    type_: 63,
    dependent_slice_segment_flag: 1,
    segment_address: 1,
    ..Default::default()
  };
  let parameters = ffi::GstH265PPS { picwidthinctbsy: 2, ..parameters };
  assert!(validate(&dependent, &parameters).is_ok());
}

#[test]
fn test_validate_address_respects_picture_bounds_and_first_segment_identity()
 {
  let parameters = ffi::GstH265PPS {
    picwidthinctbsy: 5,
    picheightinctbsy: 3,
    ..Default::default()
  };
  for first in [false, true] {
    for dependent in [false, true] {
      for address in [0, 1, 14, 15, 16, u32::MAX] {
        let slice = ffi::GstH265SliceHdr {
          first_slice_segment_in_pic_flag: u8::from(first),
          dependent_slice_segment_flag: u8::from(dependent),
          segment_address: address,
          ..Default::default()
        };
        assert_eq!(
          validate(&slice, &parameters).is_ok(),
          if first {
            !dependent && address == 0
          } else {
            (1..15).contains(&address)
          },
          "first={first}, dependent={dependent}, address={address}"
        );
      }
    }
  }
}

#[test]
fn test_validate_empty_picture_has_no_valid_segment_address() {
  for (width, height) in [(0, 0), (1, 0), (0, 1)] {
    let parameters = ffi::GstH265PPS {
      picwidthinctbsy: width,
      picheightinctbsy: height,
      ..Default::default()
    };
    let slice = ffi::GstH265SliceHdr {
      first_slice_segment_in_pic_flag: 1,
      ..Default::default()
    };
    assert!(validate(&slice, &parameters).is_err());
  }
}
