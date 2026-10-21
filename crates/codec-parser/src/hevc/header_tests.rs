//! module: header_tests; responsibility: specify owned HEVC reference
//! snapshots independently of codecparser storage lifetime.
use super::super::ffi;
use super::header::own;
fn records()
-> (ffi::GstH265NalUnit, ffi::GstH265SliceHdr, Box<ffi::GstH265SPS>) {
  // These C value records contain integers, arrays and nullable
  // pointers; zero initialization creates valid values. References
  // are borrowed explicitly rather than encoded in their raw pointer
  // fields.
  let mut nal: ffi::GstH265NalUnit = unsafe { std::mem::zeroed() };
  let mut slice: ffi::GstH265SliceHdr = unsafe { std::mem::zeroed() };
  let sps = Box::new(unsafe { std::mem::zeroed() });
  nal.type_ = 1;
  nal.temporal_id_plus1 = 1;
  slice.pic_output_flag = 1;
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
  assert_eq!(header.short, [-1, -3]);
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
  assert_eq!(header.short, [5]);
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

fn sequence() -> std::rc::Rc<crate::syntax::hevc::Sequence> {
  std::rc::Rc::new(crate::syntax::hevc::Sequence {
    geometry: crate::syntax::geometry::Geometry {
      id: 4,
      ..Default::default()
    },
    poc_bits: 4,
    layers: vec![crate::Limits::default()],
  })
}
