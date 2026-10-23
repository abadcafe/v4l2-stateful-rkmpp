//! module: codec_parser::hevc::picture; responsibility: enforce
//! coded-picture header consistency. operations: new, agrees; entities:
//! Picture, Independent, Prediction; tests: picture_tests. Only active
//! picture fields participate; slice-local coding choices do not.
#![forbid(unsafe_code)]
use std::io;

use super::super::ffi;
mod references;
#[cfg(test)]
mod references_tests;
use references::References;

// H.265 7.4.7.1: dependent segments inherit the absent portion of the
// header. Native C storage for syntax that is absent has no meaning.
pub(super) struct Picture {
  pps: usize,
  temporal_id: u8,
  nal_type: u8,
  no_prior: Option<bool>,
  independent: Option<Independent>,
}
struct Independent {
  output: bool,
  prediction: Option<Prediction>,
}
struct Prediction {
  poc: u16,
  references: References,
  temporal_mvp: bool,
}
fn no_prior(
  nal: &ffi::GstH265NalUnit,
  slice: &ffi::GstH265SliceHdr,
) -> Option<bool> {
  (16..=23)
    .contains(&nal.type_)
    .then_some(slice.no_output_of_prior_pics_flag != 0)
}
impl Picture {
  pub(super) fn new(
    nal: &ffi::GstH265NalUnit,
    slice: &ffi::GstH265SliceHdr,
  ) -> io::Result<Self> {
    let independent = if slice.dependent_slice_segment_flag == 0 {
      Some(Independent {
        output: slice.pic_output_flag != 0,
        prediction: if matches!(nal.type_, 19 | 20) {
          None
        } else {
          Some(Prediction {
            poc: slice.pic_order_cnt_lsb,
            references: References::new(slice)?,
            temporal_mvp: slice.temporal_mvp_enabled_flag != 0,
          })
        },
      })
    } else {
      None
    };
    Ok(Self {
      pps: slice.pps as usize,
      temporal_id: nal.temporal_id_plus1,
      nal_type: nal.type_,
      no_prior: no_prior(nal, slice),
      independent,
    })
  }

  /// Later segments borrow their header; only the first segment owns
  /// a snapshot. Comparison never allocates per-slice reference lists.
  pub(super) fn agrees(
    &self,
    nal: &ffi::GstH265NalUnit,
    slice: &ffi::GstH265SliceHdr,
  ) -> io::Result<bool> {
    let same = self.pps == slice.pps as usize
      && self.temporal_id == nal.temporal_id_plus1
      && self.nal_type == nal.type_
      && self.no_prior == no_prior(nal, slice);
    if !same || slice.dependent_slice_segment_flag != 0 {
      return Ok(same);
    }
    let Some(first) = &self.independent else { return Ok(false) };
    if first.output != (slice.pic_output_flag != 0) {
      return Ok(false);
    }
    let Some(prediction) = &first.prediction else { return Ok(true) };
    Ok(
      prediction.poc == slice.pic_order_cnt_lsb
        && prediction.temporal_mvp
          == (slice.temporal_mvp_enabled_flag != 0)
        && prediction.references.agrees(slice)?,
    )
  }
}
