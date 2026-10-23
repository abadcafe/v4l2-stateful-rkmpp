//! module: codec_parser::hevc::header; responsibility: detach
//! validated HEVC reference syntax from the codecparser lifetime.
//! operations: own; entities: none; tests: packet_syntax_tests and
//! service reference tests.
use std::io;

use crate::ffi;
use crate::header::{Hevc, Long, Short};
pub(super) fn own(
  nal: &ffi::GstH265NalUnit,
  slice: &ffi::GstH265SliceHdr,
  sequence: std::rc::Rc<crate::syntax::hevc::Sequence>,
  sps: &ffi::GstH265SPS,
) -> io::Result<Hevc> {
  let idr = matches!(nal.type_, 19 | 20);
  let mut short = Vec::new();
  let mut long = Vec::new();
  if !idr {
    let rps = if slice.short_term_ref_pic_set_sps_flag != 0 {
      sps
        .short_term_ref_pic_set
        .get(..usize::from(sps.num_short_term_ref_pic_sets))
        .and_then(|sets| {
          sets.get(usize::from(slice.short_term_ref_pic_set_idx))
        })
        .ok_or_else(invalid)?
    } else {
      &slice.short_term_ref_pic_sets
    };
    let negative = rps.numnegativepics as usize;
    let positive = rps.numpositivepics as usize;
    let before = rps.deltapocs0.get(..negative).ok_or_else(invalid)?;
    let before_used =
      rps.usedbycurrpics0.get(..negative).ok_or_else(invalid)?;
    let after = rps.deltapocs1.get(..positive).ok_or_else(invalid)?;
    let after_used =
      rps.usedbycurrpics1.get(..positive).ok_or_else(invalid)?;
    short = before
      .iter()
      .zip(before_used)
      .chain(after.iter().zip(after_used))
      .map(|(&delta, &used)| Short { delta, used: used != 0 })
      .collect();
    let from_sps = usize::from(slice.num_long_term_sps);
    let count = from_sps + usize::from(slice.num_long_term_pics);
    if count > slice.lt_idx_sps.len()
      || from_sps > usize::from(sps.num_long_term_ref_pics_sps)
    {
      return Err(invalid());
    }
    let mut cycle = 0u64;
    for i in 0..count {
      if i < from_sps
        && slice.lt_idx_sps[i] >= sps.num_long_term_ref_pics_sps
      {
        return Err(invalid());
      }
      let lsb = if i < from_sps {
        u32::from(
          *sps
            .lt_ref_pic_poc_lsb_sps
            .get(usize::from(slice.lt_idx_sps[i]))
            .ok_or_else(invalid)?,
        )
      } else {
        slice.poc_lsb_lt[i]
      };
      let present = slice.delta_poc_msb_present_flag[i] != 0;
      let delta = if present {
        u64::from(slice.delta_poc_msb_cycle_lt[i])
      } else {
        0
      };
      cycle = if i == 0 || i == from_sps {
        delta
      } else {
        cycle.checked_add(delta).ok_or_else(invalid)?
      };
      let used = if i < from_sps {
        *sps
          .used_by_curr_pic_lt_sps_flag
          .get(usize::from(slice.lt_idx_sps[i]))
          .ok_or_else(invalid)?
          != 0
      } else {
        *slice.used_by_curr_pic_lt_flag.get(i).ok_or_else(invalid)? != 0
      };
      long.push(Long { lsb, cycle: present.then_some(cycle), used });
    }
  }
  Ok(Hevc {
    sequence,
    kind: nal.type_,
    temporal_id: nal
      .temporal_id_plus1
      .checked_sub(1)
      .ok_or_else(invalid)?,
    no_prior: slice.no_output_of_prior_pics_flag != 0,
    display: slice.pic_output_flag != 0,
    lsb: u32::from(slice.pic_order_cnt_lsb),
    short,
    long,
  })
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid HEVC owned picture header",
  )
}
