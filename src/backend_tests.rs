//! module: backend_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_event_drains_and_resets_without_decoder
//! test_register_accepts_early_capture_but_rejects_duplicate_pool
//! test_register_defers_undersized_capture_until_layout_reset
//! test_event_packet_rejects_pool_slot_length_and_duplicate_leases
//! test_event_old_capture_recycle_cannot_acknowledge_new_layout
use std::time::{Duration, Instant};

use crate::backend::Backend;
use crate::dma::Buffer;
use crate::pool::{Direction, Pool};
use crate::protocol::{Event, Message, Reply, VERSION};

fn dispatch(
  backend: &mut Backend,
  message: Message,
) -> std::io::Result<()> {
  backend.event(crate::protocol::command::Command::try_from(message)?)
}

#[test]
fn test_invalid_input_completion_cannot_overtake_initial_picture() {
  let bytes =
    &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454];
  let buffers = vec![
    Buffer::allocate(bytes.len()).unwrap(),
    Buffer::allocate(4096).unwrap(),
  ];
  let mut access = buffers[0].write().unwrap();
  access[..bytes.len()].copy_from_slice(bytes);
  access.finish().unwrap();
  let mut access = buffers[1].write().unwrap();
  access[0] = 0;
  access.finish().unwrap();
  let mut backend = Backend::new().unwrap();
  backend
    .register(Pool::new(Direction::Output, 1, buffers).unwrap())
    .unwrap();
  for (index, length) in [(0, bytes.len()), (1, 1)] {
    dispatch(
      &mut backend,
      Message {
        version: VERSION,
        kind: Event::Packet as u32,
        epoch: 1,
        pool_id: 1,
        index,
        count: index + 1,
        bytesused: length as u32,
        codec: u32::from_le_bytes(*b"H264"),
        ..Default::default()
      },
    )
    .unwrap();
  }
  assert!(backend.take_reply().is_none());
}

#[test]
fn test_event_drains_and_resets_without_decoder() {
  let mut backend = Backend::new().unwrap();
  assert!(backend.take_reply().is_none());
  assert!(!backend.step().unwrap());
  let mut event = Message {
    version: VERSION,
    kind: Event::Drain as u32,
    ..Default::default()
  };
  dispatch(&mut backend, event).unwrap();
  assert_eq!(backend.take_reply().unwrap().kind, Reply::Eos as u32);
  event.kind = Event::Reset as u32;
  event.epoch = 3;
  event.generation = 7;
  dispatch(&mut backend, event).unwrap();
  let reply = backend.take_reply().unwrap();
  assert_eq!(
    (reply.kind, reply.epoch, reply.generation),
    (Reply::Stopped as u32, 3, 7)
  );
  assert!(backend.take_reply().is_none());
}

#[test]
fn test_register_accepts_early_capture_but_rejects_duplicate_pool() {
  for kind in [Event::Packet, Event::Recycle] {
    let mut backend = Backend::new().unwrap();
    assert!(
      dispatch(
        &mut backend,
        Message {
          version: VERSION,
          kind: kind as u32,
          count: 1,
          epoch: 1,
          ..Default::default()
        }
      )
      .is_err()
    );
    assert!(backend.take_reply().is_none());
  }
  let mut backend = Backend::new().unwrap();
  backend
    .register(
      Pool::new(
        Direction::Capture,
        1,
        vec![Buffer::allocate(4096).unwrap()],
      )
      .unwrap(),
    )
    .unwrap();
  assert!(
    backend
      .register(
        Pool::new(
          Direction::Capture,
          2,
          vec![Buffer::allocate(4096).unwrap()]
        )
        .unwrap()
      )
      .is_err()
  );
  assert!(
    dispatch(
      &mut backend,
      Message {
        version: VERSION,
        kind: Event::Reset as u32,
        flags: Message::RESET_CAPTURE_ONLY,
        ..Default::default()
      }
    )
    .is_ok()
  );
  dispatch(
    &mut backend,
    Message {
      version: VERSION,
      kind: Event::Resume as u32,
      ..Default::default()
    },
  )
  .unwrap();
  dispatch(
    &mut backend,
    Message {
      version: VERSION,
      kind: Event::Close as u32,
      ..Default::default()
    },
  )
  .unwrap();
}

#[test]
fn test_register_defers_undersized_capture_until_layout_reset() {
  let bytes = std::fs::read(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/artifacts/vp9-yuv420p.ivf"
  ))
  .unwrap();
  let size =
    u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
  let input = Buffer::allocate(size).unwrap();
  input.write().unwrap().copy_from_slice(&bytes[44..44 + size]);
  let mut backend = Backend::new().unwrap();
  backend
    .register(Pool::new(Direction::Output, 1, vec![input]).unwrap())
    .unwrap();
  let base =
    Message { version: VERSION, epoch: 1, ..Default::default() };
  dispatch(
    &mut backend,
    Message {
      kind: Event::Packet as u32,
      count: 1,
      pool_id: 1,
      codec: u32::from_le_bytes(*b"VP90"),
      bytesused: size as u32,
      ..base
    },
  )
  .unwrap();
  let deadline = Instant::now() + Duration::from_secs(5);
  let mut input_returned = false;
  loop {
    assert!(Instant::now() < deadline);
    backend.step().unwrap();
    if let Some(reply) = backend.take_reply() {
      if reply.kind == Reply::InputDone as u32 {
        input_returned = true;
      }
      if reply.kind == Reply::Layout as u32 {
        break;
      }
    }
    std::thread::yield_now();
  }
  // REQBUFS used the initial 64x64 layout; STREAMON arrived after
  // LAYOUT.
  backend
    .register(
      Pool::new(
        Direction::Capture,
        1,
        vec![Buffer::allocate(6144).unwrap()],
      )
      .unwrap(),
    )
    .unwrap();
  dispatch(
    &mut backend,
    Message {
      kind: Event::Capture as u32,
      pool_id: 1,
      count: 1,
      ..base
    },
  )
  .unwrap();
  dispatch(
    &mut backend,
    Message {
      kind: Event::Recycle as u32,
      pool_id: 1,
      count: 1,
      ..base
    },
  )
  .unwrap();
  for _ in 0..32 {
    backend.step().unwrap();
    while let Some(reply) = backend.take_reply() {
      assert_ne!(reply.kind, Reply::Frame as u32);
      input_returned |= reply.kind == Reply::InputDone as u32;
    }
  }
  dispatch(
    &mut backend,
    Message {
      kind: Event::Reset as u32,
      flags: Message::RESET_CAPTURE_ONLY,
      generation: 1,
      ..base
    },
  )
  .unwrap();
  assert_eq!(backend.take_reply().unwrap().kind, Reply::Stopped as u32);
  backend
    .register(
      Pool::new(
        Direction::Capture,
        2,
        (0..20)
          .map(|_| Buffer::allocate(256 * 128 * 2).unwrap())
          .collect(),
      )
      .unwrap(),
    )
    .unwrap();
  let capture = Message { pool_id: 2, generation: 1, ..base };
  dispatch(
    &mut backend,
    Message { kind: Event::Capture as u32, count: 20, ..capture },
  )
  .unwrap();
  for _ in 0..32 {
    backend.step().unwrap();
    assert!(
      backend.take_reply().is_none(),
      "unqueued capture became writable"
    );
  }
  for index in 0..20 {
    dispatch(
      &mut backend,
      Message {
        kind: Event::Recycle as u32,
        index,
        count: index + 1,
        ..capture
      },
    )
    .unwrap();
  }
  let mut frame_seen = false;
  loop {
    assert!(Instant::now() < deadline);
    backend.step().unwrap();
    while let Some(reply) = backend.take_reply() {
      if reply.kind == Reply::Frame as u32 {
        frame_seen = true;
      }
      if reply.kind == Reply::InputDone as u32 {
        input_returned = true;
      }
    }
    if frame_seen && input_returned {
      return;
    }
    std::thread::yield_now();
  }
}

#[test]
fn test_event_packet_rejects_pool_slot_length_and_duplicate_leases() {
  let mut backend = Backend::new().unwrap();
  let input = Buffer::allocate(8192).unwrap();
  let stream = include_bytes!("../tests/artifacts/vp9-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  input.write().unwrap()[..size]
    .copy_from_slice(&stream[44..44 + size]);
  backend
    .register(Pool::new(Direction::Output, 7, vec![input]).unwrap())
    .unwrap();
  let base = Message {
    version: VERSION,
    kind: Event::Packet as u32,
    count: 1,
    pool_id: 7,
    codec: u32::from_le_bytes(*b"VP90"),
    bytesused: size as u32,
    epoch: 1,
    ..Default::default()
  };
  for event in [
    Message { count: 0, ..base },
    Message { pool_id: 8, ..base },
    Message { index: 1, ..base },
    Message { bytesused: 0, ..base },
    Message { bytesused: 8193, ..base },
  ] {
    assert!(dispatch(&mut backend, event).is_err());
  }
  dispatch(&mut backend, base).unwrap();
  assert!(dispatch(&mut backend, base).is_err());
  assert!(
    backend
      .register(
        Pool::new(
          Direction::Output,
          8,
          vec![Buffer::allocate(8192).unwrap()]
        )
        .unwrap()
      )
      .is_err()
  );
  dispatch(
    &mut backend,
    Message {
      version: VERSION,
      kind: Event::Reset as u32,
      epoch: 2,
      generation: 3,
      ..Default::default()
    },
  )
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
  let mut capture_serial = 0;
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
      dispatch(
        &mut backend,
        Message {
          kind: Event::Packet as u32,
          count: next as u32 + 1,
          pool_id: 1,
          codec: u32::from_le_bytes(*b"VP90"),
          bytesused: packets[next].len() as u32,
          timestamp: next as u64,
          ..base(generation)
        },
      )
      .unwrap();
      next += 1;
      input_free = false;
    }
    if next == packets.len() && input_free && !draining {
      dispatch(
        &mut backend,
        Message { kind: Event::Drain as u32, ..base(generation) },
      )
      .unwrap();
      draining = true;
    }
    backend.step().unwrap();
    while let Some(reply) = backend.take_reply() {
      if reply.kind == Reply::InputDone as u32 {
        input_free = true;
      } else if reply.kind == Reply::Frame as u32 {
        frames += 1;
        capture_serial += 1;
        dispatch(
          &mut backend,
          Message {
            kind: Event::Recycle as u32,
            count: capture_serial,
            pool_id: capture_id,
            index: reply.index,
            ..base(generation)
          },
        )
        .unwrap();
      } else if reply.kind == Reply::Layout as u32 {
        layouts += 1;
        if layouts == 2 {
          assert_eq!(frames, 36);
          assert_eq!((reply.width, reply.height), (512, 256));
          // Deliberately recycle the old pool before acknowledging its
          // replacement.
          capture_serial += 1;
          dispatch(
            &mut backend,
            Message {
              kind: Event::Recycle as u32,
              count: capture_serial,
              pool_id: capture_id,
              ..base(generation)
            },
          )
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
          dispatch(
            &mut backend,
            Message {
              kind: Event::Reset as u32,
              flags: Message::RESET_CAPTURE_ONLY,
              ..base(generation)
            },
          )
          .unwrap();
        }
        capture_id += 1;
        backend
          .register(
            Pool::new(
              Direction::Capture,
              capture_id,
              (0..reply.count)
                .map(|_| Buffer::allocate(reply.size as usize).unwrap())
                .collect(),
            )
            .unwrap(),
          )
          .unwrap();
        dispatch(
          &mut backend,
          Message {
            kind: Event::Capture as u32,
            pool_id: capture_id,
            count: reply.count,
            ..base(generation)
          },
        )
        .unwrap();
        for index in 0..reply.count {
          capture_serial += 1;
          dispatch(
            &mut backend,
            Message {
              kind: Event::Recycle as u32,
              count: capture_serial,
              pool_id: capture_id,
              index,
              ..base(generation)
            },
          )
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

#[test]
fn test_event_drain_accepts_maximum_registered_inputs() {
  let codec = u32::from_le_bytes(*b"H264");
  let picture =
    &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454];
  let buffers = (0..32)
    .map(|_| {
      let buffer = Buffer::allocate(picture.len()).unwrap();
      buffer.write().unwrap().copy_from_slice(picture);
      buffer
    })
    .collect();
  let mut backend = Backend::new().unwrap();
  backend
    .register(Pool::new(Direction::Output, 1, buffers).unwrap())
    .unwrap();
  for index in 0..32 {
    dispatch(
      &mut backend,
      Message {
        version: VERSION,
        kind: Event::Packet as u32,
        pool_id: 1,
        index,
        count: index + 1,
        codec,
        bytesused: picture.len() as u32,
        ..Default::default()
      },
    )
    .unwrap();
  }
  dispatch(
    &mut backend,
    Message {
      version: VERSION,
      kind: Event::Drain as u32,
      ..Default::default()
    },
  )
  .unwrap();
}

#[test]
fn test_event_input_completion_echoes_the_queue_token_even_on_damage() {
  let input = Buffer::allocate(4096).unwrap();
  input.write().unwrap()[0] = 0xff;
  let mut backend = Backend::new().unwrap();
  backend
    .register(Pool::new(Direction::Output, 1, vec![input]).unwrap())
    .unwrap();
  dispatch(
    &mut backend,
    Message {
      version: VERSION,
      kind: Event::Packet as u32,
      epoch: 1,
      pool_id: 1,
      codec: u32::from_le_bytes(*b"H264"),
      bytesused: 1,
      count: 17,
      ..Default::default()
    },
  )
  .unwrap();
  for _ in 0..16 {
    backend.step().unwrap();
    if let Some(reply) = backend.take_reply() {
      assert_eq!(
        (reply.kind, reply.index, reply.count),
        (Reply::InputDone as u32, 0, 17)
      );
      assert_ne!(reply.flags & Message::FRAME_ERROR, 0);
      return;
    }
  }
  panic!("damaged input must be completed");
}
