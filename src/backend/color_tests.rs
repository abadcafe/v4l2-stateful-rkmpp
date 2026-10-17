//! module: backend::color_tests; responsibility: specify CICP to V4L2
//! mappings. operations: tests; entities: none; tests: representable
//! values and explicit gaps.
use super::color::Color;
#[test]
fn test_description_maps_all_representable_transfer_functions() {
  for (cicp, v4l2) in [
    (1, 1),
    (6, 1),
    (14, 1),
    (15, 1),
    (7, 4),
    (8, 5),
    (13, 2),
    (16, 7),
    (17, 6),
  ] {
    let color = Color::new(1, 1, cicp, 1);
    assert_eq!(color.transfer(), v4l2);
    assert!(color.unrepresented().is_empty());
  }
}
#[test]
fn test_description_preserves_distinct_primaries_matrices_and_ranges() {
  for (cicp, v4l2) in
    [(1, 3), (4, 5), (5, 6), (6, 1), (7, 2), (9, 10), (11, 12)]
  {
    assert_eq!(Color::new(cicp, 1, 1, 1).primaries(), v4l2);
  }
  for (cicp, v4l2) in [(1, 2), (5, 1), (6, 1), (7, 8), (9, 6), (10, 7)]
  {
    assert_eq!(Color::new(1, cicp, 1, 1).matrix(), v4l2);
  }
  assert_eq!(Color::new(9, 9, 16, 1).range(), 2);
  assert_eq!(Color::new(9, 9, 16, 2).range(), 1);
}
#[test]
fn test_description_reports_hlg_as_unrepresentable_in_current_v4l2_uapi()
 {
  let color = Color::new(9, 9, 18, 1);
  assert_eq!(color.transfer(), 0);
  assert_eq!(color.unrepresented(), vec![("transfer", 18)]);
  assert!(Color::new(2, 2, 2, 0).unrepresented().is_empty());
}
