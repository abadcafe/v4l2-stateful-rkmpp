//! module: backend::layout_tests; responsibility: identity and geometry
//! lifetime contracts, including hidden tasks and complementary fields.
use super::layout::Layouts;
use crate::picture::{
  Descriptor, Geometry, Id, Limits, Output, Part, Plan,
};
use crate::timestamp::Source;
fn descriptor() -> Descriptor {
  Descriptor {
    geometry: Some(Geometry {
      width: 1888,
      height: 1080,
      coded_width: 1920,
      coded_height: 1088,
      left: 0,
      top: 0,
    }),
    timestamp: 7,
    source: Source::Start,
    plan: Plan::default(),
  }
}
fn info(timestamp: u64) -> crate::mpp::FrameInfo {
  crate::mpp::FrameInfo {
    width: 1888,
    height: 1080,
    stride: 1920,
    rows: 1088,
    size: 0,
    format: 0,
    timestamp,
    timestamp_source: 0,
    field: 0,
    eos: false,
    info_change: false,
    failed: false,
    primaries: 0,
    matrix: 0,
    transfer: 0,
    range: 0,
  }
}
#[test]
fn test_take_preserves_reordered_identity_with_identical_timestamps() {
  let mut layouts = Layouts::default();
  let mut ids = Vec::new();
  for left in [0, 16, 32] {
    let mut d = descriptor();
    d.geometry.as_mut().unwrap().left = left;
    ids.push(layouts.push(&d).unwrap());
  }
  for i in [2, 0, 1] {
    let p = layouts.take(ids[i]).unwrap();
    assert_eq!(p.extent.unwrap().left, (i * 16) as u32);
    assert_eq!(p.timestamp, 7);
    assert_eq!(p.source, Source::Start);
  }
  assert!(layouts.take(ids[0]).is_err());
}
#[test]
fn test_peek_checks_parsed_geometry_and_resolves_decoder_geometry() {
  let mut layouts = Layouts::default();
  let mut d = descriptor();
  let id = layouts.push(&d).unwrap();
  assert_eq!(layouts.peek(id, 1888, 1080).unwrap().coded_width, 1920);
  assert!(layouts.peek(id, 64, 64).is_err());
  assert!(layouts.peek(id + 1, 1888, 1080).is_err());
  d.geometry = None;
  let id = layouts.push(&d).unwrap();
  assert_eq!(layouts.peek(id, 65, 65).unwrap().width, 65);
}
#[test]
fn test_push_rejects_out_of_bounds_geometry_without_consuming_identity()
{
  let mut layouts = Layouts::default();
  let mut d = descriptor();
  d.geometry.as_mut().unwrap().left = 33;
  assert!(layouts.push(&d).is_err());
  assert_eq!(layouts.push(&descriptor()).unwrap(), 1);
}
#[test]
fn test_complete_bufferless_error_restores_and_reclaims_provenance() {
  let mut layouts = Layouts::default();
  for _ in 0..5000 {
    let id = layouts.push(&descriptor()).unwrap();
    let mut frame = info(id);
    assert!(layouts.complete(&mut frame, false).unwrap().is_none());
    frame.failed = true;
    let p = layouts.complete(&mut frame, false).unwrap().unwrap();
    assert_eq!(
      (frame.timestamp, frame.timestamp_source),
      (7, Source::Start as u32)
    );
    assert_eq!(p.timestamp, frame.timestamp);
    assert!(layouts.take(id).is_err());
  }
}
#[test]
fn test_finish_reclaims_hidden_tasks_without_display_notifications() {
  let mut layouts = Layouts::default();
  for _ in 0..5000 {
    let mut d = descriptor();
    d.geometry = None;
    let id = layouts.push(&d).unwrap();
    layouts.finish(id);
    assert!(layouts.take(id).is_err());
  }
}
#[test]
fn test_take_pairs_only_explicit_complementary_fields() {
  for continuation in [false, true] {
    let mut layouts = Layouts::default();
    let mut d = descriptor();
    d.plan = Plan::new(
      Id::new(1),
      Part::FirstField,
      vec![],
      Limits::default(),
      true,
      Output::default(),
    );
    let first = layouts.push(&d).unwrap();
    layouts.finish(first);
    d.plan = Plan::new(
      Id::new(if continuation { 1 } else { 2 }),
      if continuation { Part::Continuation } else { Part::FirstField },
      vec![],
      Limits::default(),
      true,
      Output::default(),
    );
    let second = layouts.push(&d).unwrap();
    layouts.take(first).unwrap();
    assert_eq!(layouts.take(second).is_err(), continuation);
  }
}
#[test]
fn test_complete_info_change_does_not_consume_picture_provenance() {
  let mut layouts = Layouts::default();
  let id = layouts.push(&descriptor()).unwrap();
  let mut frame = info(id);
  frame.info_change = true;
  assert!(layouts.complete(&mut frame, false).unwrap().is_none());
  frame.info_change = false;
  assert!(layouts.complete(&mut frame, true).unwrap().is_some());
  assert!(layouts.take(id).is_err());
}

#[test]
fn test_finish_retains_pending_presentation_until_it_is_published() {
  let mut layouts = Layouts::default();
  let mut d = descriptor();
  d.plan = Plan::new(
    Id::new(1),
    Part::Frame,
    vec![],
    Limits::new(1, 0, None).unwrap(),
    false,
    Output { display: true, ..Output::default() },
  );
  let id = layouts.push(&d).unwrap();
  layouts.finish(id);
  assert_eq!(layouts.take(id).unwrap().timestamp, 7);
}

#[test]
fn test_drained_requires_every_display_and_field_to_be_completed() {
  let mut layouts = Layouts::default();
  assert!(layouts.drained().is_ok());
  for part in [Part::Frame, Part::FirstField] {
    let mut d = descriptor();
    d.plan = Plan::new(
      Id::new(1),
      part,
      vec![],
      Limits::default(),
      true,
      Output { display: true, ..Default::default() },
    );
    let id = layouts.push(&d).unwrap();
    assert!(layouts.drained().is_err());
    layouts.finish(id);
    assert!(layouts.drained().is_err());
    layouts.take(id).unwrap();
    assert!(layouts.drained().is_ok());
  }
  let id = layouts.push(&descriptor()).unwrap();
  layouts.finish(id);
  assert!(layouts.drained().is_ok());
}
