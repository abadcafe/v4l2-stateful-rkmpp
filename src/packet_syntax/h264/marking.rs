//! module: packet_syntax::h264::marking; responsibility: admit
//! consistent decoded-reference marking for each coded picture in an
//! AU (H.264 7.4.3.3). operations: default, admit; entities: Markings,
//! Marking, Operation; tests: marking_tests. Absent syntax and unused
//! C array entries never participate in equality.
use std::collections::BTreeMap;
use std::io;

use super::super::ffi;
#[derive(PartialEq, Eq)]
enum Operation {
  ForgetShort(u32),
  ForgetLong(u32),
  LongFromShort { difference: u32, index: u32 },
  LongLimit(u32),
  Reset,
  CurrentLong(u32),
}
#[derive(PartialEq, Eq)]
enum Marking {
  Disposable,
  Idr { discard: bool, long: bool },
  Sliding,
  Adaptive(Vec<Operation>),
}
impl Marking {
  fn parse(
    nal: &ffi::GstH264NalUnit,
    slice: &ffi::GstH264SliceHdr,
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
      .ok_or_else(invalid)?
      .iter()
      .map(|operation| {
        Ok(match operation.memory_management_control_operation {
          1 => Operation::ForgetShort(
            operation.difference_of_pic_nums_minus1,
          ),
          2 => Operation::ForgetLong(operation.long_term_pic_num),
          3 => Operation::LongFromShort {
            difference: operation.difference_of_pic_nums_minus1,
            index: operation.long_term_frame_idx,
          },
          4 => Operation::LongLimit(
            operation.max_long_term_frame_idx_plus1,
          ),
          5 => Operation::Reset,
          6 => Operation::CurrentLong(operation.long_term_frame_idx),
          // The codecparser's count excludes the terminating zero.
          _ => return Err(invalid()),
        })
      })
      .collect::<io::Result<Vec<_>>>()?;
    Ok(Self::Adaptive(operations))
  }
}
#[derive(Default)]
pub(super) struct Markings(BTreeMap<u8, Marking>);
impl Markings {
  pub(super) fn admit(
    &mut self,
    nal: &ffi::GstH264NalUnit,
    slice: &ffi::GstH264SliceHdr,
  ) -> io::Result<()> {
    let marking = Marking::parse(nal, slice)?;
    if let Some(previous) = self.0.get(&slice.redundant_pic_cnt) {
      if *previous != marking {
        return Err(invalid());
      }
    } else {
      self.0.insert(slice.redundant_pic_cnt, marking);
    }
    Ok(())
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid or inconsistent H264 decoded reference picture marking",
  )
}
