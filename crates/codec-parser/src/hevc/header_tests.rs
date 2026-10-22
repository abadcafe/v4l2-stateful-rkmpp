//! module: header_tests; responsibility: specify owned HEVC reference
//! snapshots independently of codecparser storage lifetime.
#![forbid(unsafe_code)]
use super::super::ffi;
use super::header::own;
fn records()
-> (ffi::GstH265NalUnit, ffi::GstH265SliceHdr, Box<ffi::GstH265SPS>) {
  // References are borrowed explicitly rather than encoded in the
  // default records' nullable raw pointer fields.
  let nal = ffi::GstH265NalUnit {
    type_: 1,
    temporal_id_plus1: 1,
    ..Default::default()
  };
  let slice =
    ffi::GstH265SliceHdr { pic_output_flag: 1, ..Default::default() };
  let sps = Box::default();
  (nal, slice, sps)
}
#[test]
fn test_own_retains_following_references_and_accumulates_long_cycles() {
  let (nal, mut slice, mut sps) = records();
  slice.short_term_ref_pic_sets.numnegativepics = 2;
  slice.short_term_ref_pic_sets.deltapocs0[..2]
    .copy_from_slice(&[-1, -3]);
  // UsedByCurrPic remains zero: following references must survive too.
  slice.num_long_term_sps = 1;
  slice.num_long_term_pics = 2;
  sps.lt_ref_pic_poc_lsb_sps[0] = 7;
  slice.poc_lsb_lt[1..3].copy_from_slice(&[9, 11]);
  slice.delta_poc_msb_present_flag[..3].fill(1);
  slice.delta_poc_msb_cycle_lt[..3].copy_from_slice(&[4, 2, 3]);
  let header = own(&nal, &slice, sequence(), &sps).unwrap();
  assert_eq!(header.sequence.geometry.id, 4);
  assert_eq!(
    header.short.iter().map(|p| (p.delta, p.used)).collect::<Vec<_>>(),
    [(-1, false), (-3, false)]
  );
  assert_eq!(
    header.long.iter().map(|p| (p.lsb, p.cycle)).collect::<Vec<_>>(),
    [(7, Some(4)), (9, Some(2)), (11, Some(5))]
  );
  sps.lt_ref_pic_poc_lsb_sps[0] = 99;
  assert_eq!(header.long[0].lsb, 7);
  assert_eq!(
    own(&nal, &slice, sequence(), &sps).unwrap().long[0].lsb,
    99
  );
}
#[test]
fn test_own_selects_sps_reference_set_and_preserves_unspecified_long_msb()
 {
  let (nal, mut slice, mut sps) = records();
  slice.short_term_ref_pic_set_sps_flag = 1;
  slice.short_term_ref_pic_set_idx = 2;
  sps.short_term_ref_pic_set[2].numpositivepics = 1;
  sps.short_term_ref_pic_set[2].deltapocs1[0] = 5;
  slice.num_long_term_pics = 1;
  slice.poc_lsb_lt[0] = 6;
  let header = own(&nal, &slice, sequence(), &sps).unwrap();
  assert_eq!(
    header.short.iter().map(|p| (p.delta, p.used)).collect::<Vec<_>>(),
    [(5, false)]
  );
  assert_eq!((header.long[0].lsb, header.long[0].cycle), (6, None));
}
#[test]
fn test_own_rejects_reference_storage_overflow() {
  let (nal, mut slice, sps) = records();
  slice.num_long_term_pics = 17;
  assert!(own(&nal, &slice, sequence(), &sps).is_err());
  slice.num_long_term_pics = 0;
  slice.short_term_ref_pic_sets.numnegativepics = 255;
  assert!(own(&nal, &slice, sequence(), &sps).is_err());
}

#[test]
fn test_own_preserves_current_usage_from_local_and_sps_reference_lists()
{
  let (nal, mut slice, mut sps) = records();
  slice.short_term_ref_pic_sets.numnegativepics = 2;
  slice.short_term_ref_pic_sets.deltapocs0[..2]
    .copy_from_slice(&[-1, -3]);
  slice.short_term_ref_pic_sets.usedbycurrpics0[0] = 1;
  slice.short_term_ref_pic_sets.numpositivepics = 1;
  slice.short_term_ref_pic_sets.deltapocs1[0] = 4;
  slice.short_term_ref_pic_sets.usedbycurrpics1[0] = 1;
  slice.num_long_term_sps = 1;
  slice.num_long_term_pics = 1;
  sps.lt_ref_pic_poc_lsb_sps[0] = 7;
  sps.used_by_curr_pic_lt_sps_flag[0] = 1;
  slice.poc_lsb_lt[1] = 9;
  let header = own(&nal, &slice, sequence(), &sps).unwrap();
  assert_eq!(
    header.short.iter().map(|p| (p.delta, p.used)).collect::<Vec<_>>(),
    [(-1, true), (-3, false), (4, true)]
  );
  assert_eq!(
    header.long.iter().map(|p| (p.lsb, p.used)).collect::<Vec<_>>(),
    [(7, true), (9, false)]
  );
  slice.short_term_ref_pic_set_sps_flag = 1;
  sps.short_term_ref_pic_set[0].numpositivepics = 1;
  sps.short_term_ref_pic_set[0].deltapocs1[0] = 5;
  sps.short_term_ref_pic_set[0].usedbycurrpics1[0] = 1;
  assert!(own(&nal, &slice, sequence(), &sps).unwrap().short[0].used);
  sps.used_by_curr_pic_lt_sps_flag[0] = 0;
  assert!(header.long[0].used);
}

fn sequence() -> std::rc::Rc<crate::syntax::hevc::Sequence> {
  std::rc::Rc::new(crate::syntax::hevc::Sequence {
    geometry: crate::syntax::geometry::Geometry {
      id: 4,
      ..Default::default()
    },
    poc_bits: 4,
    temporal_nested: true,
    layers: vec![crate::Limits::default()],
  })
}
