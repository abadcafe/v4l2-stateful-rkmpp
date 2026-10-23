//! module: hevc::slice; responsibility: admit picture-local CTB
//! addresses. operations: validate; entities: none; tests: slice_tests
//! and hevc_tests. Parsing a ceil(log2(CTB count))-bit address does not
//! enforce its bound.
#![forbid(unsafe_code)]
use std::io;

use crate::ffi;

pub(super) fn validate(
  slice: &ffi::GstH265SliceHdr,
  parameters: &ffi::GstH265PPS,
) -> io::Result<()> {
  let count = u64::from(parameters.picwidthinctbsy)
    * u64::from(parameters.picheightinctbsy);
  let first = slice.first_slice_segment_in_pic_flag != 0;
  if u64::from(slice.segment_address) >= count
    || first != (slice.segment_address == 0)
    || (first && slice.dependent_slice_segment_flag != 0)
    || (slice.dependent_slice_segment_flag == 0 && slice.type_ > 2)
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid HEVC slice type or segment address",
    ));
  }
  Ok(())
}
