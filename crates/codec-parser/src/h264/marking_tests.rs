//! module: marking_tests; responsibility: specify picture-level decoded
//! reference marking consistency (H.264 7.4.3.3). operations: tests;
//! entities: none; tests: IDR, adaptive MMCO operands/order/count,
//! redundant groups, absent fields and failed-admission atomicity.
#![forbid(unsafe_code)]
use super::super::ffi;
use super::marking::Markings;
use crate::header::{Marking, Operation};

fn slice() -> ffi::GstH264SliceHdr {
  let mut slice = ffi::GstH264SliceHdr {
    dec_ref_pic_marking: ffi::GstH264DecRefPicMarking {
      adaptive_ref_pic_marking_mode_flag: 1,
      n_ref_pic_marking: 6,
      ..Default::default()
    },
    ..Default::default()
  };
  for (i, operation) in slice.dec_ref_pic_marking.ref_pic_marking[..6]
    .iter_mut()
    .enumerate()
  {
    operation.memory_management_control_operation = (i + 1) as u8;
  }
  slice
}
fn nal(kind: u16, reference: u16) -> ffi::GstH264NalUnit {
  ffi::GstH264NalUnit {
    type_: kind,
    ref_idc: reference,
    ..Default::default()
  }
}
#[test]
fn test_admit_rejects_each_idr_flag_and_preserves_committed_group() {
  let header = nal(5, 3);
  let good = slice();
  for prior in [false, true] {
    let mut bad = good;
    if prior {
      bad.dec_ref_pic_marking.no_output_of_prior_pics_flag = 1;
    } else {
      bad.dec_ref_pic_marking.long_term_reference_flag = 1;
    }
    let mut markings = Markings::new(&header, &good).unwrap();
    assert!(markings.admit(&header, &bad).is_err());
    markings.admit(&header, &good).unwrap();
  }
}
#[test]
fn test_admit_rejects_every_active_mmco_operand() {
  let header = nal(1, 2);
  let good = slice();
  let changes: [fn(&mut ffi::GstH264SliceHdr); 7] = [
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[0]
        .difference_of_pic_nums_minus1 = 1
    },
    |s| s.dec_ref_pic_marking.ref_pic_marking[1].long_term_pic_num = 1,
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[2]
        .difference_of_pic_nums_minus1 = 1
    },
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[2].long_term_frame_idx = 1
    },
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[3]
        .max_long_term_frame_idx_plus1 = 1
    },
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[5].long_term_frame_idx = 1
    },
    |s| s.dec_ref_pic_marking.adaptive_ref_pic_marking_mode_flag = 0,
  ];
  for change in changes {
    let mut bad = good;
    change(&mut bad);
    let mut markings = Markings::new(&header, &good).unwrap();
    assert!(markings.admit(&header, &bad).is_err());
    markings.admit(&header, &good).unwrap();
  }
}
#[test]
fn test_admit_compares_mmco_order_count_and_reset_operation() {
  let header = nal(1, 2);
  let good = slice();
  let changes: [fn(&mut ffi::GstH264SliceHdr); 3] = [
    |s| s.dec_ref_pic_marking.ref_pic_marking.swap(0, 1),
    |s| s.dec_ref_pic_marking.n_ref_pic_marking -= 1,
    |s| {
      s.dec_ref_pic_marking.ref_pic_marking[4]
        .memory_management_control_operation = 4
    },
  ];
  for change in changes {
    let mut bad = good;
    change(&mut bad);
    let mut markings = Markings::new(&header, &good).unwrap();
    assert!(markings.admit(&header, &bad).is_err());
  }
}
#[test]
fn test_admit_groups_redundant_pictures_without_overwriting_primary() {
  let header = nal(1, 2);
  let primary = slice();
  let mut redundant = primary;
  redundant.redundant_pic_cnt = 1;
  // Different syntax per redundant_pic_cnt is permitted by 7.4.3.3.
  // This module compares syntax, not the eventual DPB state
  // equivalence.
  redundant.dec_ref_pic_marking.n_ref_pic_marking = 0;
  let mut markings = Markings::new(&header, &primary).unwrap();
  markings.admit(&header, &redundant).unwrap();
  markings.admit(&header, &primary).unwrap();
  markings.admit(&header, &redundant).unwrap();
  redundant.dec_ref_pic_marking.n_ref_pic_marking = 1;
  assert!(markings.admit(&header, &redundant).is_err());
  markings.admit(&header, &primary).unwrap();
}
#[test]
fn test_admit_ignores_inactive_operands_and_absent_syntax() {
  let good = slice();
  let mut other = good;
  other.dec_ref_pic_marking.ref_pic_marking[0].long_term_pic_num = 99;
  other.dec_ref_pic_marking.ref_pic_marking[4].long_term_frame_idx = 99;
  other.dec_ref_pic_marking.ref_pic_marking[9]
    .memory_management_control_operation = 99;
  other.dec_ref_pic_marking.bit_size = 99;
  other.first_mb_in_slice = 32;
  let mut markings = Markings::new(&nal(1, 2), &good).unwrap();
  markings.admit(&nal(1, 2), &other).unwrap();
  for header in [nal(5, 3), nal(1, 0)] {
    let mut markings = Markings::new(&header, &good).unwrap();
    other.dec_ref_pic_marking.n_ref_pic_marking = 255;
    markings.admit(&header, &other).unwrap();
  }
}
#[test]
fn test_new_rejects_unrepresentable_operation_and_array_count() {
  for (count, code) in [(11, 1), (1, 0), (1, 7)] {
    let mut bad = slice();
    bad.dec_ref_pic_marking.n_ref_pic_marking = count;
    bad.dec_ref_pic_marking.ref_pic_marking[0]
      .memory_management_control_operation = code;
    assert!(Markings::new(&nal(1, 2), &bad).is_err());
    Markings::new(&nal(1, 2), &slice()).unwrap();
  }
}

#[test]
fn test_finish_moves_first_valid_group_and_preserves_operation_order() {
  let header = nal(1, 2);
  let mut first = slice();
  first.redundant_pic_cnt = 2;
  let mut other = first;
  other.redundant_pic_cnt = 0;
  other.dec_ref_pic_marking.n_ref_pic_marking = 0;
  let mut markings = Markings::new(&header, &first).unwrap();
  markings.admit(&header, &other).unwrap();
  first.dec_ref_pic_marking.ref_pic_marking[0]
    .difference_of_pic_nums_minus1 = 99;
  assert!(markings.admit(&header, &first).is_err());
  assert_eq!(
    markings.finish(),
    Marking::Adaptive(vec![
      Operation::ForgetShort(0),
      Operation::ForgetLong(0),
      Operation::LongFromShort { difference: 0, index: 0 },
      Operation::LongLimit(0),
      Operation::Reset,
      Operation::CurrentLong(0),
    ])
  );
}

#[test]
fn test_new_rejects_invalid_tail_without_publishing_marking() {
  let header = nal(1, 2);
  let mut bad = slice();
  bad.dec_ref_pic_marking.ref_pic_marking[5]
    .memory_management_control_operation = 7;
  assert!(Markings::new(&header, &bad).is_err());
  let markings = Markings::new(&header, &slice()).unwrap();
  assert!(matches!(markings.finish(), Marking::Adaptive(_)));
}

#[test]
fn test_admit_malformed_existing_group_preserves_output_and_allows_retry()
 {
  let header = nal(1, 2);
  let good = slice();
  let mut markings = Markings::new(&header, &good).unwrap();
  for code in [0, 7, 255] {
    let mut bad = good;
    bad.dec_ref_pic_marking.ref_pic_marking[5]
      .memory_management_control_operation = code;
    assert!(markings.admit(&header, &bad).is_err());
    markings.admit(&header, &good).unwrap();
  }
  let Marking::Adaptive(operations) = markings.finish() else {
    panic!("adaptive input must retain its owned operations");
  };
  assert_eq!(operations.len(), 6);
  assert_eq!(operations.last(), Some(&Operation::CurrentLong(0)));
}

#[test]
fn test_admit_rejected_new_group_can_retry_without_changing_first() {
  let header = nal(1, 2);
  let mut first = slice();
  first.redundant_pic_cnt = 2;
  let mut markings = Markings::new(&header, &first).unwrap();
  let mut other = first;
  other.redundant_pic_cnt = 0;
  other.dec_ref_pic_marking.ref_pic_marking[5]
    .memory_management_control_operation = 7;
  assert!(markings.admit(&header, &other).is_err());
  other.dec_ref_pic_marking.n_ref_pic_marking = 0;
  markings.admit(&header, &other).unwrap();
  let Marking::Adaptive(operations) = markings.finish() else {
    panic!("first admitted group must retain its owned operations");
  };
  assert_eq!(operations.len(), 6);
  assert_eq!(operations.last(), Some(&Operation::CurrentLong(0)));
}
