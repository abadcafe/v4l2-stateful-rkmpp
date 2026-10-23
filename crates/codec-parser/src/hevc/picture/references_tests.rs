//! module: references_tests; responsibility: specify canonical HEVC
//! reference syntax. operations: tests; entities: none; tests:
//! selected RPS bounds, active long-term sources and optional cycles.
#![forbid(unsafe_code)]
use super::references::References;
use crate::ffi;

#[test]
fn test_agrees_sequence_selection_ignores_unselected_local_storage() {
  let mut slice = ffi::GstH265SliceHdr {
    short_term_ref_pic_set_sps_flag: 1,
    short_term_ref_pic_set_idx: 3,
    ..Default::default()
  };
  let first = References::new(&slice).unwrap();
  slice.short_term_ref_pic_sets.numnegativepics = 255;
  slice.short_term_ref_pic_sets.numpositivepics = 255;
  assert!(first.agrees(&slice).unwrap());
  slice.short_term_ref_pic_set_idx = 4;
  assert!(!first.agrees(&slice).unwrap());
}

#[test]
fn test_new_rejects_active_counts_before_indexing() {
  let slice = ffi::GstH265SliceHdr {
    num_long_term_sps: 255,
    num_long_term_pics: 255,
    ..Default::default()
  };
  assert!(References::new(&slice).is_err());
  let first =
    References::new(&ffi::GstH265SliceHdr::default()).unwrap();
  assert!(first.agrees(&slice).is_err());
}

#[test]
fn test_agrees_long_sources_and_cycles_compare_only_present_syntax() {
  let mut slice = ffi::GstH265SliceHdr {
    num_long_term_sps: 1,
    num_long_term_pics: 1,
    ..Default::default()
  };
  let first = References::new(&slice).unwrap();
  slice.poc_lsb_lt[0] = 999;
  slice.used_by_curr_pic_lt_flag[0] = 1;
  slice.lt_idx_sps[1] = 9;
  slice.delta_poc_msb_cycle_lt[..2].fill(500);
  assert!(first.agrees(&slice).unwrap());
  slice.delta_poc_msb_present_flag[1] = 1;
  assert!(!first.agrees(&slice).unwrap());
  slice.delta_poc_msb_present_flag[1] = 0;
  slice.poc_lsb_lt[1] = 1;
  assert!(!first.agrees(&slice).unwrap());
}
