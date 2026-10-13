//! module: protocol_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_message_wire_layout_matches_published_kernel_uapi
//! test_batch_wire_layout_matches_published_kernel_uapi
//! test_pool_default_has_no_owned_file_descriptors
//! test_validate_accepts_empty_and_maximum_batch_counts
//! test_validate_rejects_invalid_version_reserved_or_counts
//! test_event_and_reply_discriminants_match_wire_contract
use crate::protocol::{
  Batch, Descriptor, Event, MAX_BATCH, MAX_BUFFERS, Message, Pool,
  Reply, VERSION,
};
#[test]
fn test_message_wire_layout_matches_published_kernel_uapi() {
  assert_eq!(std::mem::size_of::<Message>(), 104);
  assert_eq!(std::mem::align_of::<Message>(), 8);
  assert_eq!(std::mem::offset_of!(Message, timestamp), 24);
  assert_eq!(std::mem::offset_of!(Message, pool_id), 88);
  assert_eq!(std::mem::offset_of!(Message, reserved), 92);
  let message = Message::default();
  assert_eq!(
    (message.version, message.kind, message.epoch, message.reserved),
    (0, 0, 0, [0; 3])
  );
  assert_eq!(Message::RESET_CAPTURE_ONLY, 1 << 16);
}
#[test]
fn test_batch_wire_layout_matches_published_kernel_uapi() {
  assert_eq!(std::mem::size_of::<Batch>(), 3408);
  assert_eq!(std::mem::offset_of!(Batch, replies), 16);
  assert_eq!(std::mem::offset_of!(Batch, events), 1680);
  assert_eq!(std::mem::offset_of!(Batch, results), 3344);
  assert_eq!(MAX_BATCH, 16);
}
#[test]
fn test_pool_default_has_no_owned_file_descriptors() {
  assert_eq!(std::mem::size_of::<Pool>(), 288);
  assert_eq!(std::mem::size_of::<Descriptor>(), 8);
  assert_eq!(std::mem::offset_of!(Pool, buffers), 32);
  assert_eq!(MAX_BUFFERS, 32);
  let pool = Pool::default();
  assert_eq!(
    (
      pool.version,
      pool.kind,
      pool.epoch,
      pool.pool_id,
      pool.count,
      pool.reserved
    ),
    (0, 0, 0, 0, 0, [0; 2])
  );
  assert!(
    pool.buffers.iter().all(|entry| entry.fd == -1 && entry.size == 0)
  );
}
#[test]
fn test_validate_accepts_empty_and_maximum_batch_counts() {
  for reply_count in [0, MAX_BATCH as u32] {
    for event_count in [0, MAX_BATCH as u32] {
      Batch {
        version: VERSION,
        reply_count,
        event_count,
        ..Default::default()
      }
      .validate()
      .unwrap();
    }
  }
}
#[test]
fn test_validate_rejects_invalid_version_reserved_or_counts() {
  for (version, reply_count, event_count, reserved) in [
    (0, 0, 0, 0),
    (VERSION + 1, 0, 0, 0),
    (VERSION, 17, 0, 0),
    (VERSION, 0, 17, 0),
    (VERSION, u32::MAX, 0, 0),
    (VERSION, 0, u32::MAX, 0),
    (VERSION, 0, 0, 1),
  ] {
    assert_eq!(
      Batch {
        version,
        reply_count,
        event_count,
        reserved,
        ..Default::default()
      }
      .validate()
      .unwrap_err()
      .kind(),
      std::io::ErrorKind::InvalidData
    );
  }
}
#[test]
fn test_event_and_reply_discriminants_match_wire_contract() {
  assert_eq!(
    [
      Event::Packet,
      Event::Capture,
      Event::Recycle,
      Event::Drain,
      Event::Reset,
      Event::Close,
      Event::Resume
    ]
    .map(|v| v as u32),
    [1, 2, 3, 4, 5, 6, 7]
  );
  assert_eq!(
    [
      Reply::Layout,
      Reply::InputDone,
      Reply::Frame,
      Reply::Eos,
      Reply::Stopped,
      Reply::Failed
    ]
    .map(|v| v as u32),
    [32, 33, 34, 35, 36, 37]
  );
}

#[test]
fn test_all_public_wire_fields_and_constants_match_compiled_c_header() {
  use std::io::Write;
  use std::process::{Command, Stdio};
  let mut source =
    String::from("#include <stddef.h>\n#include \"bridge_uapi.h\"\n");
  macro_rules! layout {
    ($rust:ty, $c:literal, $($field:ident => $name:literal),+ $(,)?) => {{
      source.push_str(&format!(
        "_Static_assert(sizeof(struct {}) == {}, \"size\");\n_Static_assert(_Alignof(struct {}) == {}, \"alignment\");\n",
        $c, std::mem::size_of::<$rust>(), $c, std::mem::align_of::<$rust>()
      ));
      $(source.push_str(&format!(
        "_Static_assert(offsetof(struct {}, {}) == {}, \"offset {}\");\n",
        $c, $name, std::mem::offset_of!($rust, $field), $name
      ));)+
    }};
  }
  layout!(Message, "rb_message",
    version => "version", kind => "type", epoch => "epoch",
    index => "index", flags => "flags", timestamp => "timestamp",
    bytesused => "bytesused", codec => "codec", width => "width",
    height => "height", stride => "stride", rows => "rows",
    size => "size", format => "format", count => "count",
    generation => "generation", colorspace => "colorspace",
    xfer_func => "xfer_func", ycbcr_enc => "ycbcr_enc",
    quantization => "quantization", pool_id => "pool_id", reserved => "reserved",
  );
  layout!(Batch, "rb_batch", version => "version", reply_count => "reply_count",
    event_count => "event_count", reserved => "reserved", replies => "replies",
    events => "events", results => "results",
  );
  layout!(Pool, "rb_pool", version => "version", kind => "type", epoch => "epoch",
    pool_id => "pool_id", count => "count", reserved => "reserved", buffers => "buffers",
  );
  layout!(Descriptor, "rb_registered_buffer", fd => "fd", size => "size");
  let constants = [
    ("RB_VERSION", VERSION as usize),
    ("RB_BATCH_MAX", MAX_BATCH),
    ("RB_MAX_BUFFERS", MAX_BUFFERS),
    ("RB_RESET_CAPTURE_ONLY", Message::RESET_CAPTURE_ONLY as usize),
    ("RB_PACKET", Event::Packet as usize),
    ("RB_CAPTURE", Event::Capture as usize),
    ("RB_RECYCLE", Event::Recycle as usize),
    ("RB_DRAIN", Event::Drain as usize),
    ("RB_RESET", Event::Reset as usize),
    ("RB_CLOSE", Event::Close as usize),
    ("RB_RESUME", Event::Resume as usize),
    ("RB_LAYOUT", Reply::Layout as usize),
    ("RB_INPUT_DONE", Reply::InputDone as usize),
    ("RB_FRAME", Reply::Frame as usize),
    ("RB_EOS", Reply::Eos as usize),
    ("RB_STOPPED", Reply::Stopped as usize),
    ("RB_FAILED", Reply::Failed as usize),
  ];
  for (name, value) in constants {
    source.push_str(&format!(
      "_Static_assert({name} == {value}, \"constant {name}\");\n"
    ));
  }
  let mut compiler = Command::new("cc")
    .args([
      "-x",
      "c",
      "-std=c11",
      "-Wall",
      "-Wextra",
      "-Werror",
      "-fsyntax-only",
      "-I",
    ])
    .arg(
      std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("kernel"),
    )
    .arg("-")
    .stdin(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  compiler.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
  let output = compiler.wait_with_output().unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
}
