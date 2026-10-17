//! Public decoded-to-CAPTURE format contract, independent of the
//! parser.
use super::format::Format;
use crate::backend::layout::Extent;
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
fn extent() -> Extent {
  Extent {
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
    let format = Format::new(info, extent()).unwrap();
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
    assert_eq!(format.extent().coded_width, 256);
    assert!(!format.changed(info, extent()));
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
    assert!(Format::new(info, extent()).is_err(), "{info:?}");
  }
  for geometry in [
    Extent { left: u32::MAX, ..extent() },
    Extent { top: 2, ..extent() },
    Extent { coded_width: 0, ..extent() },
    Extent { coded_height: 8193, ..extent() },
  ] {
    assert!(Format::new(frame(), geometry).is_err());
  }
}
#[test]
fn test_changed_tracks_storage_and_color_but_not_frame_provenance() {
  let format = Format::new(frame(), extent()).unwrap();
  for info in [
    FrameInfo { size: 65536, ..frame() },
    FrameInfo { matrix: 9, ..frame() },
    FrameInfo { transfer: 16, ..frame() },
    FrameInfo { primaries: 9, ..frame() },
    FrameInfo { range: 2, ..frame() },
    FrameInfo { field: 1, ..frame() },
  ] {
    assert!(format.changed(info, extent()), "{info:?}");
  }
  assert!(!format.changed(
    FrameInfo { timestamp: 100, eos: true, failed: true, ..frame() },
    extent()
  ));
  assert!(format.changed(frame(), Extent { left: 0, ..extent() }));
}

#[test]
fn test_changed_compares_field_arrangement_not_picture_parity() {
  // V4L2 ALTERNATE is one format; TOP/BOTTOM belong to buffers.
  let arrangements = [0, 1, 2, 3, 3, 5];
  for (first, arrangement) in arrangements.iter().enumerate() {
    let format = Format::new(
      FrameInfo { field: first as u32, ..frame() },
      extent(),
    )
    .unwrap();
    for (next, expected) in arrangements.iter().enumerate() {
      assert_eq!(
        format.changed(
          FrameInfo { field: next as u32, ..frame() },
          extent()
        ),
        arrangement != expected,
        "field {first} -> {next}"
      );
    }
  }
}
