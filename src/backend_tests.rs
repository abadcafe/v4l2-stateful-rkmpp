//! module: backend_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_event_validates_events_and_resets_without_decoder
//! test_register_and_event_reject_missing_pool_or_decoder_preconditions
//! test_event_packet_rejects_pool_slot_length_and_duplicate_leases
//! test_event_old_capture_recycle_cannot_acknowledge_new_layout
use std::time::{Duration, Instant};

use crate::backend::Backend;
use crate::dma::Buffer;
use crate::pool::{Direction, Pool};
use crate::protocol::{Event, Message, Reply, VERSION};

#[test]
fn test_event_validates_events_and_resets_without_decoder() {
  let mut backend = Backend::new().unwrap();
  assert!(backend.take_reply().is_none());
  assert!(!backend.step().unwrap());
  assert!(backend.event(Message::default()).is_err());
  let mut event = Message {
    version: VERSION,
    kind: Event::Drain as u32,
    ..Default::default()
  };
  assert!(backend.event(event).is_err());
  event.kind = 999;
  assert!(backend.event(event).is_err());
  event.kind = Event::Reset as u32;
  event.epoch = 3;
  event.generation = 7;
  backend.event(event).unwrap();
  let reply = backend.take_reply().unwrap();
  assert_eq!(
    (reply.kind, reply.epoch, reply.generation),
    (Reply::Stopped as u32, 3, 7)
  );
  assert!(backend.take_reply().is_none());
}

#[test]
fn test_register_and_event_reject_missing_pool_or_decoder_preconditions()
 {
  for kind in [Event::Packet, Event::Recycle, Event::Drain] {
    let mut backend = Backend::new().unwrap();
    assert!(
      backend
        .event(Message {
          version: VERSION,
          kind: kind as u32,
          epoch: 1,
          ..Default::default()
        })
        .is_err()
    );
    assert!(backend.take_reply().is_none());
  }
  let mut backend = Backend::new().unwrap();
  assert!(
    backend
      .register(
        Pool::new(
          Direction::Capture,
          1,
          vec![Buffer::allocate(4096).unwrap()]
        )
        .unwrap()
      )
      .is_err()
  );
  assert!(
    backend
      .event(Message {
        version: VERSION,
        kind: Event::Reset as u32,
        flags: Message::RESET_CAPTURE_ONLY,
        ..Default::default()
      })
      .is_err()
  );
  backend
    .event(Message {
      version: VERSION,
      kind: Event::Resume as u32,
      ..Default::default()
    })
    .unwrap();
  backend
    .event(Message {
      version: VERSION,
      kind: Event::Close as u32,
      ..Default::default()
    })
    .unwrap();
}

#[test]
fn test_event_packet_rejects_pool_slot_length_and_duplicate_leases() {
  let mut backend = Backend::new().unwrap();
  backend
    .register(
      Pool::new(
        Direction::Output,
        7,
        vec![Buffer::allocate(4096).unwrap()],
      )
      .unwrap(),
    )
    .unwrap();
  let base = Message {
    version: VERSION,
    kind: Event::Packet as u32,
    pool_id: 7,
    codec: u32::from_le_bytes(*b"H264"),
    bytesused: 16,
    epoch: 1,
    ..Default::default()
  };
  for event in [
    Message { pool_id: 8, ..base },
    Message { index: 1, ..base },
    Message { bytesused: 0, ..base },
    Message { bytesused: 4097, ..base },
  ] {
    assert!(backend.event(event).is_err());
  }
  backend.event(base).unwrap();
  assert!(backend.event(base).is_err());
  assert!(
    backend
      .register(
        Pool::new(
          Direction::Output,
          8,
          vec![Buffer::allocate(4096).unwrap()]
        )
        .unwrap()
      )
      .is_err()
  );
  backend
    .event(Message {
      version: VERSION,
      kind: Event::Reset as u32,
      epoch: 2,
      generation: 3,
      ..Default::default()
    })
    .unwrap();
  let reply = backend.take_reply().unwrap();
  assert_eq!(
    (reply.kind, reply.epoch, reply.generation),
    (Reply::Stopped as u32, 2, 3)
  );
}

#[test]
fn test_event_old_capture_recycle_cannot_acknowledge_new_layout() {
  let stream = std::fs::read(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("tests/artifacts/vp9-10-resize.ivf"),
  )
  .unwrap();
  let mut packets = Vec::new();
  let mut offset = 32;
  while offset < stream.len() {
    let size = u32::from_le_bytes(
      stream[offset..offset + 4].try_into().unwrap(),
    ) as usize;
    packets.push(&stream[offset + 12..offset + 12 + size]);
    offset += 12 + size;
  }
  assert_eq!(packets.len(), 48);
  let input = Buffer::allocate(65536).unwrap();
  let mut backend = Backend::new().unwrap();
  backend
    .register(
      Pool::new(Direction::Output, 1, vec![input.clone()]).unwrap(),
    )
    .unwrap();
  let wake = backend.notification();
  let mut next = 0;
  let mut input_free = true;
  let mut draining = false;
  let mut frames = 0;
  let mut layouts = 0;
  let mut capture_id = 0;
  let mut generation = 1;
  let mut eos = false;
  let deadline = Instant::now() + Duration::from_secs(10);
  let base = |generation| Message {
    version: VERSION,
    epoch: 1,
    generation,
    ..Default::default()
  };
  while !eos {
    assert!(Instant::now() < deadline, "source change stalled");
    if input_free && next < packets.len() {
      input.write().unwrap()[..packets[next].len()]
        .copy_from_slice(packets[next]);
      backend
        .event(Message {
          kind: Event::Packet as u32,
          pool_id: 1,
          codec: u32::from_le_bytes(*b"VP90"),
          bytesused: packets[next].len() as u32,
          timestamp: next as u64,
          ..base(generation)
        })
        .unwrap();
      next += 1;
      input_free = false;
    }
    if next == packets.len() && input_free && !draining {
      backend
        .event(Message {
          kind: Event::Drain as u32,
          ..base(generation)
        })
        .unwrap();
      draining = true;
    }
    backend.step().unwrap();
    while let Some(reply) = backend.take_reply() {
      if reply.kind == Reply::InputDone as u32 {
        input_free = true;
      } else if reply.kind == Reply::Frame as u32 {
        frames += 1;
        backend
          .event(Message {
            kind: Event::Recycle as u32,
            pool_id: capture_id,
            index: reply.index,
            ..base(generation)
          })
          .unwrap();
      } else if reply.kind == Reply::Layout as u32 {
        layouts += 1;
        if layouts == 2 {
          assert_eq!(frames, 36);
          assert_eq!((reply.width, reply.height), (512, 256));
          // Deliberately recycle the old pool before acknowledging its
          // replacement.
          backend
            .event(Message {
              kind: Event::Recycle as u32,
              pool_id: capture_id,
              ..base(generation)
            })
            .unwrap();
          let settle = Instant::now() + Duration::from_millis(250);
          while Instant::now() < settle {
            backend.step().unwrap();
            while let Some(waiting) = backend.take_reply() {
              assert_eq!(
                waiting.kind,
                Reply::InputDone as u32,
                "new-layout frame escaped into old capture pool"
              );
              input_free = true;
            }
            wake.wait(Duration::from_millis(10)).unwrap();
            wake.drain().unwrap();
          }
          generation += 1;
          backend
            .event(Message {
              kind: Event::Reset as u32,
              flags: Message::RESET_CAPTURE_ONLY,
              ..base(generation)
            })
            .unwrap();
        }
        capture_id += 1;
        backend
          .register(
            Pool::new(
              Direction::Capture,
              capture_id,
              (0..8)
                .map(|_| Buffer::allocate(reply.size as usize).unwrap())
                .collect(),
            )
            .unwrap(),
          )
          .unwrap();
        backend
          .event(Message {
            kind: Event::Capture as u32,
            pool_id: capture_id,
            count: 8,
            ..base(generation)
          })
          .unwrap();
        for index in 0..8 {
          backend
            .event(Message {
              kind: Event::Recycle as u32,
              pool_id: capture_id,
              index,
              ..base(generation)
            })
            .unwrap();
        }
      } else if reply.kind == Reply::Eos as u32 {
        eos = true;
      } else {
        assert_eq!(reply.kind, Reply::Stopped as u32);
      }
    }
    wake.wait(Duration::from_millis(1)).unwrap();
    wake.drain().unwrap();
  }
  assert_eq!((layouts, frames), (2, 48));
}
