//! module: frontend decoder tests; responsibility: verify admitted AV1
//! tasks complete through MPP. operations: Rust tests; entities: none;
//! tests: synchronous progress and per-input publication.
use dma_buf::memory::Buffer;
use rkmpp::{Decoder, Input, Packet, Payload, Submission};

use crate::timestamp::Source;
#[test]
fn test_packet_av1_internal_progress_does_not_require_an_external_wakeup()
 {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/artifacts/av1-yuv420p.ivf"
  ));
  let codec = u32::from_le_bytes(*b"AV01");
  let mut frontend = crate::frontend::Frontend::new(codec).unwrap();
  let mut decoder = Decoder::new(codec, || Ok(())).unwrap();
  let mut offset = 32;
  let mut frames = 0;
  let mut identity = 0;
  while offset < stream.len() {
    let size = u32::from_le_bytes(
      stream[offset..offset + 4].try_into().unwrap(),
    ) as usize;
    let bytes = &stream[offset + 12..offset + 12 + size];
    for task in frontend.admit(bytes, 0, Source::End, 32).unwrap().tasks
    {
      identity += 1;
      let packet = packet(bytes, task.payload, identity);
      let mut consumed = false;
      for _ in 0..64 {
        if !consumed {
          let state = decoder.packet(&packet).unwrap();
          consumed = state == Submission::Consumed;
        }
        while let Some(frame) = decoder.frame().unwrap() {
          let info = frame.info();
          assert!(!info.failed);
          if info.info_change {
            decoder.configure_output(false).unwrap();
            let buffers: Vec<_> = (0..18)
              .map(|slot| (slot, Buffer::allocate(info.size).unwrap()))
              .collect();
            decoder.import_capture(&buffers, 18).unwrap();
            decoder.ready().unwrap();
          } else if frame.plane().unwrap().is_some() {
            frames += 1;
          }
        }
        if consumed && decoder.idle() {
          break;
        }
      }
      assert!(consumed && decoder.idle());
    }
    offset += 12 + size;
  }
  assert_eq!(frames, 36);
}

#[test]
fn test_packet_av1_completes_each_display_without_future_input() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/artifacts/av1-yuv420p.ivf"
  ));
  let codec = u32::from_le_bytes(*b"AV01");
  let mut frontend = crate::frontend::Frontend::new(codec).unwrap();
  let mut decoder = Decoder::new(codec, || Ok(())).unwrap();
  let mut offset = 32;
  let mut frames = 0;
  let mut identity = 0;
  for expected in 1..=6 {
    let size = u32::from_le_bytes(
      stream[offset..offset + 4].try_into().unwrap(),
    ) as usize;
    let bytes = &stream[offset + 12..offset + 12 + size];
    for task in frontend.admit(bytes, 0, Source::End, 32).unwrap().tasks
    {
      identity += 1;
      let packet = packet(bytes, task.payload, identity);
      let mut consumed = false;
      for _ in 0..64 {
        if !consumed {
          consumed =
            decoder.packet(&packet).unwrap() == Submission::Consumed;
        }
        while let Some(frame) = decoder.frame().unwrap() {
          let info = frame.info();
          assert!(!info.failed);
          if info.info_change {
            decoder.configure_output(false).unwrap();
            let buffers: Vec<_> = (0..18)
              .map(|slot| (slot, Buffer::allocate(info.size).unwrap()))
              .collect();
            decoder.import_capture(&buffers, 18).unwrap();
            decoder.ready().unwrap();
          } else {
            assert!(
              frame.plane().unwrap().is_some(),
              "internal publication markers must not escape the MPP \
               adapter"
            );
            frames += 1;
          }
        }
        if consumed && decoder.idle() {
          break;
        }
      }
      assert!(consumed && decoder.idle());
    }
    assert_eq!(
      frames, expected,
      "each completed input must deliver its own images"
    );
    offset += 12 + size;
  }
}

fn packet(bytes: &[u8], payload: Payload, identity: u64) -> Packet {
  let buffer = Buffer::allocate(bytes.len()).unwrap();
  let mut write = buffer.write().unwrap();
  write[..bytes.len()].copy_from_slice(bytes);
  write.finish().unwrap();
  let input = Input::new(buffer).unwrap();
  let access = input.prepare(0, bytes.len()).unwrap();
  access.packet(payload, identity, Source::End as i64).unwrap()
}
