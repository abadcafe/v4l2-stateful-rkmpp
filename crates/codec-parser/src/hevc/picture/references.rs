//! module: hevc::picture::references; responsibility: compare active
//! slice reference syntax independently of unused native storage.
//! operations: References new/agrees; entities: References, ShortTerm,
//! LongTerm, LongSource; tests: references_tests and picture_tests.
#![forbid(unsafe_code)]
use std::io;

use crate::ffi;

enum ShortTerm {
  Sequence(u8),
  Explicit { negative: Vec<(i32, bool)>, positive: Vec<(i32, bool)> },
}
#[derive(PartialEq, Eq)]
enum LongSource {
  Sequence(u8),
  Explicit { lsb: u32, used: bool },
}
#[derive(PartialEq, Eq)]
struct LongTerm {
  source: LongSource,
  cycle: Option<u32>,
}
pub(super) struct References {
  short: ShortTerm,
  long: Vec<LongTerm>,
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "HEVC picture reference count exceeds syntax storage",
  )
}
fn counts(
  slice: &ffi::GstH265SliceHdr,
) -> io::Result<(usize, usize, usize)> {
  let rps = &slice.short_term_ref_pic_sets;
  let (negative, positive) =
    if slice.short_term_ref_pic_set_sps_flag == 0 {
      (rps.numnegativepics as usize, rps.numpositivepics as usize)
    } else {
      (0, 0)
    };
  let long = usize::from(slice.num_long_term_sps)
    + usize::from(slice.num_long_term_pics);
  if negative > rps.deltapocs0.len()
    || positive > rps.deltapocs1.len()
    || long > slice.lt_idx_sps.len()
  {
    return Err(invalid());
  }
  Ok((negative, positive, long))
}
fn long(slice: &ffi::GstH265SliceHdr, i: usize) -> LongTerm {
  LongTerm {
    source: if i < usize::from(slice.num_long_term_sps) {
      LongSource::Sequence(slice.lt_idx_sps[i])
    } else {
      LongSource::Explicit {
        lsb: slice.poc_lsb_lt[i],
        used: slice.used_by_curr_pic_lt_flag[i] != 0,
      }
    },
    cycle: (slice.delta_poc_msb_present_flag[i] != 0)
      .then_some(slice.delta_poc_msb_cycle_lt[i]),
  }
}
fn short<'a>(
  deltas: &'a [i32],
  used: &'a [u8],
  count: usize,
) -> impl Iterator<Item = (i32, bool)> + 'a {
  deltas
    .iter()
    .copied()
    .zip(used.iter().map(|&used| used != 0))
    .take(count)
}
impl References {
  pub(super) fn new(slice: &ffi::GstH265SliceHdr) -> io::Result<Self> {
    let (negative, positive, count) = counts(slice)?;
    let rps = &slice.short_term_ref_pic_sets;
    Ok(Self {
      short: if slice.short_term_ref_pic_set_sps_flag != 0 {
        ShortTerm::Sequence(slice.short_term_ref_pic_set_idx)
      } else {
        ShortTerm::Explicit {
          negative: short(
            &rps.deltapocs0,
            &rps.usedbycurrpics0,
            negative,
          )
          .collect(),
          positive: short(
            &rps.deltapocs1,
            &rps.usedbycurrpics1,
            positive,
          )
          .collect(),
        }
      },
      long: (0..count).map(|i| long(slice, i)).collect(),
    })
  }

  pub(super) fn agrees(
    &self,
    slice: &ffi::GstH265SliceHdr,
  ) -> io::Result<bool> {
    let (negative, positive, count) = counts(slice)?;
    let rps = &slice.short_term_ref_pic_sets;
    let same = match &self.short {
      ShortTerm::Sequence(index) => {
        slice.short_term_ref_pic_set_sps_flag != 0
          && *index == slice.short_term_ref_pic_set_idx
      }
      ShortTerm::Explicit { negative: before, positive: after } => {
        slice.short_term_ref_pic_set_sps_flag == 0
          && before.len() == negative
          && after.len() == positive
          && before.iter().copied().eq(short(
            &rps.deltapocs0,
            &rps.usedbycurrpics0,
            negative,
          ))
          && after.iter().copied().eq(short(
            &rps.deltapocs1,
            &rps.usedbycurrpics1,
            positive,
          ))
      }
    };
    Ok(
      same
        && self.long.len() == count
        && self
          .long
          .iter()
          .enumerate()
          .all(|(i, previous)| *previous == long(slice, i)),
    )
  }
}
