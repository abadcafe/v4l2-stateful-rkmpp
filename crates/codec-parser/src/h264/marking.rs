//! module: codec_parser::h264::marking; responsibility: admit
//! consistent decoded-reference marking for each coded picture in an
//! AU (H.264 7.4.3.3). operations: new, admit, finish; entities:
//! Markings, Syntax, Marking, Operation; tests: marking_tests. Absent
//! syntax and unused C array entries never participate in equality.
use std::collections::BTreeMap;
use std::io;

use super::super::ffi;
use crate::header::{Marking, Operation};
enum Syntax<'a> {
  Disposable,
  Idr { discard: bool, long: bool },
  Sliding,
  Adaptive(&'a [ffi::GstH264RefPicMarking]),
}
impl<'a> Syntax<'a> {
  fn read(
    nal: &ffi::GstH264NalUnit,
    slice: &'a ffi::GstH264SliceHdr,
  ) -> io::Result<Self> {
    let marking = &slice.dec_ref_pic_marking;
    if nal.ref_idc == 0 {
      return Ok(Self::Disposable);
    }
    if nal.type_ == 5 {
      return Ok(Self::Idr {
        discard: marking.no_output_of_prior_pics_flag != 0,
        long: marking.long_term_reference_flag != 0,
      });
    }
    if marking.adaptive_ref_pic_marking_mode_flag == 0 {
      return Ok(Self::Sliding);
    }
    let operations = marking
      .ref_pic_marking
      .get(..usize::from(marking.n_ref_pic_marking))
      .ok_or_else(invalid)?;
    Ok(Self::Adaptive(operations))
  }

  fn own(self) -> io::Result<Marking> {
    Ok(match self {
      Self::Disposable => Marking::Disposable,
      Self::Idr { discard, long } => Marking::Idr { discard, long },
      Self::Sliding => Marking::Sliding,
      Self::Adaptive(operations) => Marking::Adaptive(
        operations.iter().map(operation).collect::<io::Result<_>>()?,
      ),
    })
  }

  fn agrees(self, stored: &Marking) -> io::Result<bool> {
    match (self, stored) {
      (Self::Disposable, Marking::Disposable)
      | (Self::Sliding, Marking::Sliding) => Ok(true),
      (
        Self::Idr { discard, long },
        Marking::Idr { discard: old_discard, long: old_long },
      ) => Ok(discard == *old_discard && long == *old_long),
      (Self::Adaptive(operations), Marking::Adaptive(previous)) => {
        if operations.len() != previous.len() {
          return Ok(false);
        }
        for (raw, expected) in operations.iter().zip(previous) {
          if operation(raw)? != *expected {
            return Ok(false);
          }
        }
        Ok(true)
      }
      _ => Ok(false),
    }
  }
}
fn operation(raw: &ffi::GstH264RefPicMarking) -> io::Result<Operation> {
  Ok(match raw.memory_management_control_operation {
    1 => Operation::ForgetShort(raw.difference_of_pic_nums_minus1),
    2 => Operation::ForgetLong(raw.long_term_pic_num),
    3 => Operation::LongFromShort {
      difference: raw.difference_of_pic_nums_minus1,
      index: raw.long_term_frame_idx,
    },
    4 => Operation::LongLimit(raw.max_long_term_frame_idx_plus1),
    5 => Operation::Reset,
    6 => Operation::CurrentLong(raw.long_term_frame_idx),
    // The codecparser's count excludes the terminating zero.
    _ => return Err(invalid()),
  })
}
pub(super) struct Markings {
  groups: BTreeMap<u8, Marking>,
  first: u8,
}
impl Markings {
  pub(super) fn new(
    nal: &ffi::GstH264NalUnit,
    slice: &ffi::GstH264SliceHdr,
  ) -> io::Result<Self> {
    let marking = Syntax::read(nal, slice)?.own()?;
    let first = slice.redundant_pic_cnt;
    let mut groups = BTreeMap::new();
    groups.insert(first, marking);
    Ok(Self { groups, first })
  }

  pub(super) fn admit(
    &mut self,
    nal: &ffi::GstH264NalUnit,
    slice: &ffi::GstH264SliceHdr,
  ) -> io::Result<()> {
    let syntax = Syntax::read(nal, slice)?;
    use std::collections::btree_map::Entry;
    match self.groups.entry(slice.redundant_pic_cnt) {
      Entry::Vacant(entry) => {
        entry.insert(syntax.own()?);
      }
      Entry::Occupied(entry) => {
        if !syntax.agrees(entry.get())? {
          return Err(invalid());
        }
      }
    }
    Ok(())
  }

  /// Transfer the first admitted picture's owned syntax after all
  /// slices have been checked. Construction guarantees this group
  /// exists.
  pub(super) fn finish(mut self) -> Marking {
    self.groups.remove(&self.first).unwrap()
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid or inconsistent H264 decoded reference picture marking",
  )
}
