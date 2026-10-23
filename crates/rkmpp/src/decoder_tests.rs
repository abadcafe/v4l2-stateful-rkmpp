//! module: rkmpp::decoder_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_new_all_supported_codecs_have_nonblocking_empty_frame_and_reset
//! test_new_rejects_unknown_codec
//! test_import_capture_and_ready_require_output_group
use dma_buf::memory::Buffer;

#[test]
fn test_new_retains_callback_state_until_close_even_after_moving_decoder()
 {
  let state = std::sync::Arc::new(());
  let retained = std::sync::Arc::downgrade(&state);
  let decoder = Decoder::new(u32::from_le_bytes(*b"H264"), move || {
    let _ = &state;
    Ok(())
  })
  .unwrap();
  assert!(retained.upgrade().is_some());
  let mut decoder = Box::new(decoder);
  decoder.reset().unwrap();
  assert!(retained.upgrade().is_some());
  decoder.close().unwrap();
  assert!(retained.upgrade().is_none());
}

use super::decoder::{Decoder, Submission};
use crate::{Input, Packet, Payload};

#[test]
fn test_packet_unknown_h264_level_preserves_bounded_picture_output() {
  // External MPP 1.1.0 failure: a complete IDR with level_idc=255 is
  // consumed without an image, while the otherwise identical levels
  // 11 and 10 yield both images. This uses Decoder directly, without
  // bridge admission, picture bookkeeping or presentation scheduling.
  // Keep the required output strict; do not rewrite the SPS for MPP.
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/h264-yuv420p.h264"
  ));
  let original = &stream[..4454];
  for level in [11, 10, 255] {
    let mut first = original.to_vec();
    let sps =
      first.windows(4).position(|b| b == [0, 0, 1, 0x67]).unwrap();
    assert_eq!(first[sps + 6], 11);
    first[sps + 6] = level;
    let mut decoder =
      Decoder::new(u32::from_le_bytes(*b"H264"), || Ok(())).unwrap();
    let mut timestamps = Vec::new();
    for (index, data) in
      [first.as_slice(), original].into_iter().enumerate()
    {
      let input =
        packet(data, Payload::Input(0..data.len()), index as u64);
      let mut consumed = false;
      for _ in 0..64 {
        if !consumed {
          consumed =
            decoder.packet(&input).unwrap() == Submission::Consumed;
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
            timestamps.push(info.pts);
          }
        }
        if consumed && decoder.idle() {
          break;
        }
      }
      assert!(consumed && decoder.idle());
    }
    decoder.close().unwrap();
    assert_eq!(timestamps, [0, 1], "advertised level {level}");
  }
}

#[test]
fn test_packet_repeated_reference_publishes_each_presentation() {
  let stream = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/artifacts/vp9-yuv420p.ivf"
  ));
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"VP90"), || Ok(())).unwrap();
  let mut timestamps = Vec::new();
  for (index, data) in
    [key, &[0x88], &[0x88], &[0x88]].into_iter().enumerate()
  {
    let packet =
      packet(data, Payload::Input(0..data.len()), index as u64);
    let mut accepted = false;
    for _ in 0..64 {
      if !accepted {
        accepted =
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
        } else if frame.plane().unwrap().is_some() {
          timestamps.push(info.pts);
        }
      }
      if accepted && decoder.idle() {
        break;
      }
    }
    assert!(accepted && decoder.idle());
  }
  decoder.close().unwrap();
  // A show_existing_frame input has no new hardware decode task, but
  // each occurrence must still publish the reference with its own PTS.
  // MPP 1.1.0 currently queues these internally without publishing them
  // on the synchronous no-task path. Keep this strict regression red.
  assert_eq!(timestamps, [0, 1, 2, 3]);
}

#[test]
fn test_new_all_supported_codecs_have_nonblocking_empty_frame_and_reset()
 {
  for codec in [*b"H264", *b"HEVC", *b"VP80", *b"VP90", *b"AV01"] {
    let mut decoder =
      Decoder::new(u32::from_le_bytes(codec), || Ok(())).unwrap();
    assert!(decoder.frame().unwrap().is_none());
    assert!(
      decoder.idle(),
      "an empty decoder must have no outstanding work: {codec:?}"
    );
    assert!(!decoder.has_output());
    decoder.reset().unwrap();
    decoder.close().unwrap();
  }
}

#[test]
fn test_new_rejects_unknown_codec() {
  assert_eq!(
    Decoder::new(0, || Ok(())).err().unwrap().kind(),
    std::io::ErrorKind::InvalidInput
  );
}

#[test]
fn test_import_capture_and_ready_require_output_group() {
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"H264"), || Ok(())).unwrap();
  assert!(
    decoder
      .import_capture(&[(0, Buffer::allocate(4096).unwrap())], 1)
      .is_err()
  );
  assert!(decoder.ready().is_err());
  decoder.configure_output(false).unwrap();
  assert!(decoder.has_output());
  decoder
    .import_capture(&[(0, Buffer::allocate(4096).unwrap())], 1)
    .unwrap();
  decoder.configure_output(true).unwrap();
  assert!(decoder.has_output());
  decoder.reset().unwrap();
  decoder.close().unwrap();
}

fn packet(bytes: &[u8], payload: Payload, identity: u64) -> Packet {
  let buffer = Buffer::allocate(bytes.len()).unwrap();
  let mut write = buffer.write().unwrap();
  write[..bytes.len()].copy_from_slice(bytes);
  write.finish().unwrap();
  let input = Input::new(buffer).unwrap();
  let access = input.prepare(0, bytes.len()).unwrap();
  access.packet(payload, identity, 0).unwrap()
}
