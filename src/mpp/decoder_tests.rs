//! module: mpp::decoder_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_new_all_supported_codecs_have_nonblocking_empty_frame_and_reset
//! test_new_rejects_unknown_codec
//! test_import_capture_and_ready_require_output_group
use super::decoder::{Decoder, Submission};
use crate::dma::Buffer;
use crate::mpp::Packet;
use crate::notify::Notify;
use crate::timestamp::Source;

#[test]
fn test_packet_repeated_reference_publishes_each_presentation() {
  let stream = include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let key = &stream[44..44 + size];
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"VP90"), Notify::new().unwrap())
      .unwrap();
  let mut timestamps = Vec::new();
  for (index, data) in
    [key, &[0x88], &[0x88], &[0x88]].into_iter().enumerate()
  {
    let packet =
      Packet::owned(data.to_vec(), index as u64, Source::End).unwrap();
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
          timestamps.push(info.timestamp);
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
      Decoder::new(u32::from_le_bytes(codec), Notify::new().unwrap())
        .unwrap();
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
    Decoder::new(0, Notify::new().unwrap()).err().unwrap().kind(),
    std::io::ErrorKind::InvalidInput
  );
}

#[test]
fn test_import_capture_and_ready_require_output_group() {
  let mut decoder =
    Decoder::new(u32::from_le_bytes(*b"H264"), Notify::new().unwrap())
      .unwrap();
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

#[test]
fn test_packet_av1_internal_progress_does_not_require_an_external_wakeup()
 {
  let stream = include_bytes!("../../tests/artifacts/av1-yuv420p.ivf");
  let codec = u32::from_le_bytes(*b"AV01");
  let mut frontend = crate::frontend::Frontend::new(codec).unwrap();
  let mut decoder =
    Decoder::new(codec, Notify::new().unwrap()).unwrap();
  let mut offset = 32;
  let mut frames = 0;
  let mut identity = 0;
  while offset < stream.len() {
    let size = u32::from_le_bytes(
      stream[offset..offset + 4].try_into().unwrap(),
    ) as usize;
    let bytes = &stream[offset + 12..offset + 12 + size];
    for task in frontend.admit(bytes, 0, Source::End).unwrap().tasks {
      let data = match task.payload {
        crate::submission::Payload::Input(range) => {
          bytes[range].to_vec()
        }
        crate::submission::Payload::Owned(bytes) => bytes,
      };
      identity += 1;
      let buffer = Buffer::allocate(data.len()).unwrap();
      let mut write = buffer.write().unwrap();
      write[..data.len()].copy_from_slice(&data);
      write.finish().unwrap();
      let mut input = Packet::input(buffer).unwrap();
      let access =
        input.prepare(0, data.len(), identity, Source::End).unwrap();
      let packet =
        access.packet(0..data.len(), identity, Source::End).unwrap();
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
  let stream = include_bytes!("../../tests/artifacts/av1-yuv420p.ivf");
  let codec = u32::from_le_bytes(*b"AV01");
  let mut frontend = crate::frontend::Frontend::new(codec).unwrap();
  let mut decoder =
    Decoder::new(codec, Notify::new().unwrap()).unwrap();
  let mut offset = 32;
  let mut frames = 0;
  let mut identity = 0;
  for expected in 1..=6 {
    let size = u32::from_le_bytes(
      stream[offset..offset + 4].try_into().unwrap(),
    ) as usize;
    let bytes = &stream[offset + 12..offset + 12 + size];
    for task in frontend.admit(bytes, 0, Source::End).unwrap().tasks {
      let data = match task.payload {
        crate::submission::Payload::Input(range) => {
          bytes[range].to_vec()
        }
        crate::submission::Payload::Owned(bytes) => bytes,
      };
      identity += 1;
      let packet = Packet::owned(data, identity, Source::End).unwrap();
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
