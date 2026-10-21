//! module: protocol::command_tests; responsibility: specify typed
//! command admission. operations: tests; entities: none; tests:
//! complete domain values, malformed envelopes, invalid queue
//! identities, extents, codecs and control flags.
use super::command::{Action, Command, Stop};
use super::{Event, Message, VERSION};
fn message(kind: Event) -> Message {
  Message {
    version: VERSION,
    kind: kind as u32,
    epoch: 9,
    generation: 7,
    pool_id: 4,
    index: 3,
    count: 1,
    bytesused: 1,
    codec: u32::from_le_bytes(*b"H264"),
    timestamp: u64::MAX,
    ..Default::default()
  }
}
#[test]
fn test_try_from_admits_typed_payloads_and_routing() {
  for event in [
    Event::Packet,
    Event::Capture,
    Event::Recycle,
    Event::Drain,
    Event::Reset,
    Event::Close,
    Event::Resume,
  ] {
    let command = Command::try_from(message(event)).unwrap();
    assert_eq!(command.kind(), event);
    assert_eq!((command.route.epoch, command.route.generation), (9, 7));
    match command.action {
      Action::Packet {
        pool,
        index,
        token,
        extent,
        timestamp,
        codec,
        source,
      } => {
        assert_eq!(
          (
            pool.get(),
            index.get(),
            token.get(),
            extent.offset(),
            extent.length(),
            timestamp
          ),
          (4, 3, 1, 0, 1, u64::MAX)
        );
        assert_eq!(codec.fourcc(), u32::from_le_bytes(*b"H264"));
        assert_eq!(source, crate::timestamp::Source::End);
      }
      Action::Capture { count, reuse, pool } => {
        assert_eq!((count.get(), reuse, pool.get()), (1, false, 4))
      }
      Action::Recycle { pool, index, token } => {
        assert_eq!((pool.get(), index.get(), token.get()), (4, 3, 1))
      }
      Action::Reset { token, scope } => {
        assert_eq!(token.get(), 1);
        assert!(matches!(scope, Stop::All));
      }
      Action::Drain | Action::Close | Action::Resume => {}
    }
  }
}
#[test]
fn test_try_from_rejects_invalid_envelope_for_every_event() {
  for kind in [
    Event::Packet,
    Event::Capture,
    Event::Recycle,
    Event::Drain,
    Event::Reset,
    Event::Close,
    Event::Resume,
  ] {
    let base = message(kind);
    for invalid in [
      Message { version: 0, ..base },
      Message { version: VERSION + 1, ..base },
      Message { reserved: [1], ..base },
      Message { reserved: [u32::MAX], ..base },
    ] {
      assert!(Command::try_from(invalid).is_err());
    }
  }
  for kind in [0, 8, 32, 38, u32::MAX] {
    assert!(
      Command::try_from(Message { kind, ..message(Event::Close) })
        .is_err()
    );
  }
}
#[test]
fn test_try_from_rejects_packet_before_pool_registration() {
  let base = message(Event::Packet);
  for invalid in [
    Message { count: 0, ..base },
    Message { pool_id: 0, ..base },
    Message { index: 32, ..base },
    Message { codec: 0, ..base },
    Message { bytesused: 0, ..base },
    Message { bytesused: 16 * 1024 * 1024 + 1, ..base },
    Message { data_offset: u32::MAX, ..base },
    Message { flags: 0x20000, ..base },
    Message { flags: 1, ..base },
  ] {
    assert!(Command::try_from(invalid).is_err(), "{invalid:?}");
  }
  let maximum = Command::try_from(Message {
    bytesused: 16 * 1024 * 1024,
    flags: 0x10000,
    index: 31,
    count: u32::MAX,
    ..base
  })
  .unwrap();
  assert!(matches!(maximum.action, Action::Packet { .. }));
}
#[test]
fn test_try_from_rejects_invalid_capture_recycle_and_reset_contracts() {
  for kind in [Event::Capture, Event::Recycle, Event::Reset] {
    assert!(
      Command::try_from(Message { count: 0, ..message(kind) }).is_err()
    );
  }
  for invalid in [
    Message { count: 33, ..message(Event::Capture) },
    Message { flags: 2, ..message(Event::Capture) },
    Message { pool_id: 0, ..message(Event::Recycle) },
    Message { index: 32, ..message(Event::Recycle) },
    Message { flags: 1, ..message(Event::Reset) },
    Message {
      flags: Message::RESET_ABORT_DRAIN,
      ..message(Event::Reset)
    },
  ] {
    assert!(Command::try_from(invalid).is_err(), "{invalid:?}");
  }
  assert!(matches!(
    Command::try_from(Message {
      flags: Message::RESET_CAPTURE_ONLY | Message::RESET_ABORT_DRAIN,
      ..message(Event::Reset)
    })
    .unwrap()
    .action,
    Action::Reset { scope: Stop::Capture { abort_drain: true }, .. }
  ));
}
