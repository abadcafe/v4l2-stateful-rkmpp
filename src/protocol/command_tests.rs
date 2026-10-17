//! module: protocol::command_tests; responsibility: validated event
//! admission. operations: tests; entities: none; tests: every event,
//! rejected envelopes and preservation of routing metadata.
use super::command::Command;
use super::{Event, Message, VERSION};

#[test]
fn test_try_from_admits_every_event_without_losing_routing_or_payload()
{
  for kind in [
    Event::Packet,
    Event::Capture,
    Event::Recycle,
    Event::Drain,
    Event::Reset,
    Event::Close,
    Event::Resume,
  ] {
    let wire = Message {
      version: VERSION,
      kind: kind as u32,
      epoch: 9,
      generation: 7,
      pool_id: 4,
      index: 3,
      timestamp: u64::MAX,
      ..Default::default()
    };
    let command = Command::try_from(wire).unwrap();
    assert_eq!(command.kind(), kind);
    let admitted = command.message();
    assert_eq!(
      (
        admitted.epoch,
        admitted.generation,
        admitted.pool_id,
        admitted.index,
        admitted.timestamp
      ),
      (9, 7, 4, 3, u64::MAX)
    );
  }
}

#[test]
fn test_try_from_rejects_invalid_envelope_for_every_event() {
  for kind in 1..=7 {
    let base = Message { version: VERSION, kind, ..Default::default() };
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
      Command::try_from(Message {
        version: VERSION,
        kind,
        ..Default::default()
      })
      .is_err()
    );
  }
}
