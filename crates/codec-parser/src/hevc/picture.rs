//! module: codec_parser::hevc::picture; responsibility: enforce
//! coded-picture header consistency. operations: new, agrees; entities:
//! Picture, Independent; tests: picture_tests. Only present/inferred
//! picture fields participate; independent slice-local coding choices
//! do not.
use std::io;

use super::super::ffi;

// H.265 7.4.7.1: these fields belong to the coded picture, not to an
// individual slice. Dependent segments inherit only the absent portion;
// their PPS/NAL identity and no-prior-output flag still must agree.
pub(super) struct Picture {
  pps: usize,
  temporal_id: u8,
  nal_type: u8,
  no_prior: bool,
  independent: Option<Independent>,
}
struct Independent {
  output: bool,
  poc: u16,
  sps_rps: bool,
  rps_index: u8,
  negative: Vec<(i32, u8)>,
  positive: Vec<(i32, u8)>,
  long_sps: u8,
  long_pics: u8,
  long: Vec<(u8, u32, u8, u8, u32)>,
  temporal_mvp: bool,
}
fn references(slice: &ffi::GstH265SliceHdr) -> io::Result<()> {
  let rps = &slice.short_term_ref_pic_sets;
  let long_count = usize::from(slice.num_long_term_sps)
    + usize::from(slice.num_long_term_pics);
  if slice.dependent_slice_segment_flag == 0
    && (rps.numnegativepics as usize > rps.deltapocs0.len()
      || rps.numpositivepics as usize > rps.deltapocs1.len()
      || long_count > slice.lt_idx_sps.len())
  {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "HEVC picture reference count exceeds syntax storage",
    ));
  }
  Ok(())
}
impl Picture {
  pub(super) fn new(
    nal: &ffi::GstH265NalUnit,
    slice: &ffi::GstH265SliceHdr,
  ) -> io::Result<Self> {
    references(slice)?;
    let independent =
      (slice.dependent_slice_segment_flag == 0).then(|| {
        let rps = &slice.short_term_ref_pic_sets;
        Independent {
          output: slice.pic_output_flag != 0,
          poc: slice.pic_order_cnt_lsb,
          sps_rps: slice.short_term_ref_pic_set_sps_flag != 0,
          rps_index: slice.short_term_ref_pic_set_idx,
          negative: rps
            .deltapocs0
            .iter()
            .copied()
            .zip(rps.usedbycurrpics0)
            .take(rps.numnegativepics as usize)
            .collect(),
          positive: rps
            .deltapocs1
            .iter()
            .copied()
            .zip(rps.usedbycurrpics1)
            .take(rps.numpositivepics as usize)
            .collect(),
          long_sps: slice.num_long_term_sps,
          long_pics: slice.num_long_term_pics,
          long: (0..usize::from(slice.num_long_term_sps)
            + usize::from(slice.num_long_term_pics))
            .map(|i| {
              (
                slice.lt_idx_sps[i],
                slice.poc_lsb_lt[i],
                slice.used_by_curr_pic_lt_flag[i],
                slice.delta_poc_msb_present_flag[i],
                slice.delta_poc_msb_cycle_lt[i],
              )
            })
            .collect(),
          temporal_mvp: slice.temporal_mvp_enabled_flag != 0,
        }
      });
    Ok(Self {
      pps: slice.pps as usize,
      temporal_id: nal.temporal_id_plus1,
      nal_type: nal.type_,
      no_prior: slice.no_output_of_prior_pics_flag != 0,
      independent,
    })
  }

  /// Compare a later segment while its parsed header is borrowed. Only
  /// the first segment needs an owned snapshot across native slice
  /// destruction.
  pub(super) fn agrees(
    &self,
    nal: &ffi::GstH265NalUnit,
    slice: &ffi::GstH265SliceHdr,
  ) -> io::Result<bool> {
    references(slice)?;
    let same = self.pps == slice.pps as usize
      && self.temporal_id == nal.temporal_id_plus1
      && self.nal_type == nal.type_
      && self.no_prior == (slice.no_output_of_prior_pics_flag != 0);
    if !same || slice.dependent_slice_segment_flag != 0 {
      return Ok(same);
    }
    let Some(first) = &self.independent else {
      return Ok(false);
    };
    let rps = &slice.short_term_ref_pic_sets;
    let negative = rps.numnegativepics as usize;
    let positive = rps.numpositivepics as usize;
    let long = usize::from(slice.num_long_term_sps)
      + usize::from(slice.num_long_term_pics);
    Ok(
      first.output == (slice.pic_output_flag != 0)
        && first.poc == slice.pic_order_cnt_lsb
        && first.sps_rps
          == (slice.short_term_ref_pic_set_sps_flag != 0)
        && first.rps_index == slice.short_term_ref_pic_set_idx
        && first.long_sps == slice.num_long_term_sps
        && first.long_pics == slice.num_long_term_pics
        && first.temporal_mvp == (slice.temporal_mvp_enabled_flag != 0)
        && first.negative.len() == negative
        && first.negative.iter().copied().eq(
          rps
            .deltapocs0
            .iter()
            .copied()
            .zip(rps.usedbycurrpics0.iter().copied())
            .take(negative),
        )
        && first.positive.len() == positive
        && first.positive.iter().copied().eq(
          rps
            .deltapocs1
            .iter()
            .copied()
            .zip(rps.usedbycurrpics1.iter().copied())
            .take(positive),
        )
        && first.long.len() == long
        && first.long.iter().copied().eq((0..long).map(|i| {
          (
            slice.lt_idx_sps[i],
            slice.poc_lsb_lt[i],
            slice.used_by_curr_pic_lt_flag[i],
            slice.delta_poc_msb_present_flag[i],
            slice.delta_poc_msb_cycle_lt[i],
          )
        })),
    )
  }
}
