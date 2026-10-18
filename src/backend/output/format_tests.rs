//! Public decoded-to-CAPTURE format contract, independent of the
//! parser.
use super::format::Format;
use crate::backend::layout::Requirements;
use crate::mpp::FrameInfo;

fn frame() -> FrameInfo {
  FrameInfo {
    width: 255,
    height: 127,
    stride: 256,
    rows: 128,
    size: 49152,
    format: 0,
    timestamp: 0,
    timestamp_source: 0,
    field: 0,
    eos: false,
    info_change: false,
    failed: false,
    primaries: 1,
    matrix: 1,
    transfer: 1,
    range: 1,
  }
}
fn requirements() -> Requirements {
  Requirements {
    minimum: 18,
    width: 255,
    height: 127,
    coded_width: 256,
    coded_height: 128,
    left: 1,
    top: 1,
  }
}
#[test]
fn test_new_preserves_crop_and_allocates_nv12_for_both_depths() {
  for ten in [false, true] {
    let info = FrameInfo { format: u32::from(ten), ..frame() };
    let format = Format::new(info, requirements()).unwrap();
    let m = format.message();
    assert_eq!(
      (m.width, m.height, m.crop_left, m.crop_top),
      (255, 127, 1, 1)
    );
    assert_eq!(
      (m.stride, m.rows, m.size, m.format),
      (256, 128, 49152, 1)
    );
    assert_eq!(format.ten(), ten);
    assert_eq!(format.requirements().coded_width, 256);
    assert!(!format.changed(info, requirements()));
  }
}
#[test]
fn test_new_rejects_invalid_storage_and_unrepresentable_geometry() {
  for info in [
    FrameInfo { width: 0, ..frame() },
    FrameInfo { width: 256, ..frame() },
    FrameInfo { stride: 255, ..frame() },
    FrameInfo { rows: 127, ..frame() },
    FrameInfo { rows: 129, ..frame() },
    FrameInfo { size: 49151, ..frame() },
    FrameInfo { size: usize::MAX, ..frame() },
    FrameInfo { field: 6, ..frame() },
    FrameInfo { format: 2, ..frame() },
  ] {
    assert!(Format::new(info, requirements()).is_err(), "{info:?}");
  }
  for geometry in [
    Requirements { minimum: 17, ..requirements() },
    Requirements { minimum: 33, ..requirements() },
    Requirements { left: u32::MAX, ..requirements() },
    Requirements { top: 2, ..requirements() },
    Requirements { coded_width: 0, ..requirements() },
    Requirements { coded_height: 8193, ..requirements() },
  ] {
    assert!(Format::new(frame(), geometry).is_err());
  }
}
#[test]
fn test_changed_tracks_storage_and_color_but_not_frame_provenance() {
  let format = Format::new(frame(), requirements()).unwrap();
  for info in [
    FrameInfo { size: 65536, ..frame() },
    FrameInfo { matrix: 9, ..frame() },
    FrameInfo { transfer: 16, ..frame() },
    FrameInfo { primaries: 9, ..frame() },
    FrameInfo { range: 2, ..frame() },
    FrameInfo { field: 1, ..frame() },
  ] {
    assert!(format.changed(info, requirements()), "{info:?}");
  }
  assert!(!format.changed(
    FrameInfo { timestamp: 100, eos: true, failed: true, ..frame() },
    requirements()
  ));
  assert!(
    format.changed(frame(), Requirements { left: 0, ..requirements() })
  );
}

#[test]
fn test_changed_compares_field_arrangement_not_picture_parity() {
  // V4L2 ALTERNATE is one format; TOP/BOTTOM belong to buffers.
  let arrangements = [0, 1, 2, 3, 3, 5];
  for (first, arrangement) in arrangements.iter().enumerate() {
    let format = Format::new(
      FrameInfo { field: first as u32, ..frame() },
      requirements(),
    )
    .unwrap();
    for (next, expected) in arrangements.iter().enumerate() {
      assert_eq!(
        format.changed(
          FrameInfo { field: next as u32, ..frame() },
          requirements()
        ),
        arrangement != expected,
        "field {first} -> {next}"
      );
    }
  }
}

#[test]
fn test_new_announces_external_reference_and_display_storage_budget() {
  let geometry = Requirements { minimum: 32, ..requirements() };
  assert_eq!(
    Format::new(frame(), geometry).unwrap().message().count,
    32
  );
  let ten = FrameInfo { format: 1, ..frame() };
  assert_eq!(Format::new(ten, geometry).unwrap().message().count, 18);
  assert!(
    Format::new(frame(), requirements())
      .unwrap()
      .changed(frame(), geometry)
  );
}
