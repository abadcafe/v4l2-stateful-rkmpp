//! module: h264::slice; responsibility: admit picture-local slice
//! addresses. operations: validate; entities: none; tests: slice_tests
//! and h264_tests. Native header parsing does not enforce H.264 7.4.3's
//! macroblock bound.
#![forbid(unsafe_code)]
use std::io;

use crate::ffi;
use crate::syntax::geometry::Geometry;

pub(super) fn validate(
  slice: &ffi::GstH264SliceHdr,
  geometry: Geometry,
  adaptive: bool,
) -> io::Result<()> {
  let field = slice.field_pic_flag != 0;
  let macroblocks = u64::from(geometry.coded_width / 16)
    * u64::from(geometry.coded_height / 16)
    / if field { 2 } else { 1 };
  // In MBAFF frame pictures first_mb_in_slice addresses a pair. Field
  // pictures always address individual macroblocks, even in an
  // adaptive sequence.
  let first = u64::from(slice.first_mb_in_slice)
    * if adaptive && !field { 2 } else { 1 };
  if slice.type_ > 9 || first >= macroblocks {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid H264 slice type or macroblock address",
    ));
  }
  Ok(())
}
