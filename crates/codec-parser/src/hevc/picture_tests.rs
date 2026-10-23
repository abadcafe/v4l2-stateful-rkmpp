//! module: picture_tests; responsibility: specify picture-wide HEVC
//! consistency through the header comparison API. operations: tests;
//! entities: none; tests: independent/dependent invariants, active RPS,
//! absent arrays and invalid reference counts (H.265 7.4.7.1).
#![forbid(unsafe_code)]
use super::super::ffi;
use super::picture::Picture;

fn header() -> (ffi::GstH265NalUnit, ffi::GstH265SliceHdr) {
  // Scalar/array-only C input records; null PPS is an opaque identity
  // here. The owning syntax validator separately validates PPS
  // existence.
  let nal = ffi::GstH265NalUnit {
    type_: 21,
    temporal_id_plus1: 1,
    ..Default::default()
  };
  let mut slice = ffi::GstH265SliceHdr {
    pic_output_flag: 1,
    num_long_term_sps: 1,
    num_long_term_pics: 1,
    short_term_ref_pic_sets: ffi::GstH265ShortTermRefPicSet {
      numnegativepics: 1,
      numpositivepics: 1,
      ..Default::default()
    },
    ..Default::default()
  };
  slice.delta_poc_msb_present_flag[..2].fill(1);
  (nal, slice)
}

#[test]
fn test_agrees_rejects_every_picture_wide_independent_field() {
  let (nal, slice) = header();
  let first = Picture::new(&nal, &slice).unwrap();
  let changes: [fn(&mut ffi::GstH265SliceHdr); 16] = [
    |s| s.no_output_of_prior_pics_flag ^= 1,
    |s| s.pic_output_flag ^= 1,
    |s| s.pic_order_cnt_lsb += 1,
    |s| s.short_term_ref_pic_set_sps_flag ^= 1,
    |s| s.num_long_term_sps += 1,
    |s| s.num_long_term_pics += 1,
    |s| s.lt_idx_sps[0] += 1,
    |s| s.poc_lsb_lt[1] += 1,
    |s| s.used_by_curr_pic_lt_flag[1] ^= 1,
    |s| s.delta_poc_msb_present_flag[0] ^= 1,
    |s| s.delta_poc_msb_cycle_lt[0] += 1,
    |s| s.temporal_mvp_enabled_flag ^= 1,
    |s| s.short_term_ref_pic_sets.deltapocs0[0] += 1,
    |s| s.short_term_ref_pic_sets.deltapocs1[0] += 1,
    |s| s.short_term_ref_pic_sets.usedbycurrpics0[0] ^= 1,
    |s| s.short_term_ref_pic_sets.usedbycurrpics1[0] ^= 1,
  ];
  for (index, change) in changes.into_iter().enumerate() {
    let mut other = slice;
    change(&mut other);
    assert!(!first.agrees(&nal, &other).unwrap(), "field {index}");
  }
}

#[test]
fn test_agrees_ignores_inactive_reference_representations() {
  let (nal, mut slice) = header();
  slice.delta_poc_msb_present_flag[0] = 0;
  let first = Picture::new(&nal, &slice).unwrap();
  let mut other = slice;
  other.short_term_ref_pic_set_idx = 9;
  other.poc_lsb_lt[0] = 500;
  other.used_by_curr_pic_lt_flag[0] = 1;
  other.lt_idx_sps[1] = 7;
  other.delta_poc_msb_cycle_lt[0] = 999;
  assert!(first.agrees(&nal, &other).unwrap());

  slice.short_term_ref_pic_set_sps_flag = 1;
  let first = Picture::new(&nal, &slice).unwrap();
  other = slice;
  other.short_term_ref_pic_sets.numnegativepics = 255;
  other.short_term_ref_pic_sets.numpositivepics = 255;
  assert!(first.agrees(&nal, &other).unwrap());
  other.short_term_ref_pic_set_idx += 1;
  assert!(!first.agrees(&nal, &other).unwrap());
}

#[test]
fn test_agrees_idr_ignores_absent_prediction_fields() {
  let (mut nal, slice) = header();
  nal.type_ = 20;
  let first = Picture::new(&nal, &slice).unwrap();
  let mut other = slice;
  other.pic_order_cnt_lsb = 1023;
  other.short_term_ref_pic_set_sps_flag = 1;
  other.short_term_ref_pic_set_idx = 63;
  other.num_long_term_sps = 255;
  other.num_long_term_pics = 255;
  other.temporal_mvp_enabled_flag = 1;
  assert!(first.agrees(&nal, &other).unwrap());
}

#[test]
fn test_agrees_dependent_segment_inherits_only_absent_fields() {
  let (nal, slice) = header();
  let first = Picture::new(&nal, &slice).unwrap();
  let mut dependent = slice;
  dependent.dependent_slice_segment_flag = 1;
  dependent.pic_output_flag = 0;
  dependent.pic_order_cnt_lsb = 99;
  assert!(first.agrees(&nal, &dependent).unwrap());
  dependent.no_output_of_prior_pics_flag = 1;
  assert!(!first.agrees(&nal, &dependent).unwrap());
  dependent.no_output_of_prior_pics_flag = 0;
  dependent.pps = std::ptr::dangling_mut();
  assert!(!first.agrees(&nal, &dependent).unwrap());
  dependent.pps = slice.pps;
  for independent in [slice, dependent] {
    assert!(first.agrees(&nal, &independent).unwrap());
    let mut other = nal;
    other.temporal_id_plus1 += 1;
    assert!(!first.agrees(&other, &independent).unwrap());
    other = nal;
    other.type_ += 1;
    assert!(!first.agrees(&other, &independent).unwrap());
  }
}

#[test]
fn test_agrees_ignores_slice_local_fields_and_inactive_array_entries() {
  let (nal, slice) = header();
  let first = Picture::new(&nal, &slice).unwrap();
  let mut other = slice;
  other.segment_address = 8;
  other.type_ = 1;
  other.qp_delta = 4;
  other.lt_idx_sps[15] = 9;
  other.short_term_ref_pic_sets.deltapocs0[15] = 8;
  assert!(first.agrees(&nal, &other).unwrap());
}

#[test]
fn test_agrees_rejects_invalid_borrowed_counts_without_changing_first()
{
  let (nal, slice) = header();
  let first = Picture::new(&nal, &slice).unwrap();
  let changes: [fn(&mut ffi::GstH265SliceHdr); 3] = [
    |s| s.num_long_term_pics = 16,
    |s| s.short_term_ref_pic_sets.numnegativepics = 17,
    |s| s.short_term_ref_pic_sets.numpositivepics = 17,
  ];
  for change in changes {
    let mut other = slice;
    change(&mut other);
    assert!(first.agrees(&nal, &other).is_err());
    assert!(first.agrees(&nal, &slice).unwrap());
    // Dependent segments do not carry these reference lists.
    other.dependent_slice_segment_flag = 1;
    assert!(first.agrees(&nal, &other).unwrap());
  }
}

#[test]
fn test_new_rejects_out_of_range_reference_counts() {
  let (nal, slice) = header();
  let changes: [fn(&mut ffi::GstH265SliceHdr); 3] = [
    |s| s.num_long_term_pics = 16,
    |s| s.short_term_ref_pic_sets.numnegativepics = 17,
    |s| s.short_term_ref_pic_sets.numpositivepics = 17,
  ];
  for change in changes {
    let mut other = slice;
    change(&mut other);
    assert!(Picture::new(&nal, &other).is_err());
  }
}
