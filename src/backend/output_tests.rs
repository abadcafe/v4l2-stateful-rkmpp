//! module: backend::output_tests; responsibility: CAPTURE transaction
//! public contract.
use std::num::NonZeroU32;

use dma_buf::Buffer;

use super::output::{Output, Status};
#[test]
fn test_register_duplicate_pool_and_unknown_recycle_do_not_change_ownership()
 {
  let mut output = Output::new(crate::quantize::Factory::default());
  assert_eq!(output.id(), 0);
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  assert!(
    output
      .register(2, vec![Buffer::allocate(4096).unwrap()], None)
      .is_err()
  );
  assert_eq!(output.id(), 1);
  assert!(
    output.recycle(2, 0, NonZeroU32::new(1).unwrap(), None).is_err()
  );
  assert!(
    output.recycle(1, 1, NonZeroU32::new(1).unwrap(), None).is_err()
  );
  output.start(1, false, None).unwrap();
  output.recycle(1, 0, NonZeroU32::new(1).unwrap(), None).unwrap();
  assert_eq!(output.status(), Status::Active);
}
#[test]
fn test_stop_requires_explicit_start_before_replacement_can_run() {
  let mut output = Output::new(crate::quantize::Factory::default());
  output.stop(None).unwrap();
  assert_eq!(output.status(), Status::Stopped);
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  assert_eq!(output.status(), Status::Stopped);
  output.start(1, false, None).unwrap();
  assert_ne!(output.status(), Status::Stopped);
  assert!(output.start(1, true, None).is_err());
}

#[test]
fn test_register_rejects_invalid_identity_and_capacity_atomically() {
  let mut output = Output::new(crate::quantize::Factory::default());
  for (id, count) in [(0, 1), (1, 0), (1, 33)] {
    let buffers =
      (0..count).map(|_| Buffer::allocate(4096).unwrap()).collect();
    assert!(output.register(id, buffers, None).is_err());
    assert_eq!(output.id(), 0);
    assert_eq!(output.status(), Status::Unconfigured);
  }
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  output.start(1, false, None).unwrap();
  assert_eq!(output.status(), Status::Active);
}

#[test]
fn test_start_invalid_count_preserves_stopped_pool() {
  let mut output = Output::new(crate::quantize::Factory::default());
  output.stop(None).unwrap();
  output
    .register(1, vec![Buffer::allocate(4096).unwrap()], None)
    .unwrap();
  for count in [0, 33] {
    assert!(output.start(count, false, None).is_err());
    assert_eq!(output.status(), Status::Stopped);
    assert_eq!(output.id(), 1);
  }
  output.start(1, false, None).unwrap();
  assert_eq!(output.status(), Status::Active);
}

#[test]
fn test_start_reuse_validates_storage_before_changing_decoder() {
  use rkmpp::{Decoder, Input};

  use super::frame::FrameInput;
  use super::layout::Requirements;
  use crate::timestamp::Source;

  for (count, bytes) in [(18, 1_048_576), (32, 4096)] {
    let mut decoder =
      Decoder::new(u32::from_le_bytes(*b"VP90"), || Ok(())).unwrap();
    let stream =
      include_bytes!("../../tests/artifacts/vp9-yuv420p.ivf");
    let size =
      u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
    let buffer = Buffer::allocate(size).unwrap();
    let mut write = buffer.write().unwrap();
    write[..size].copy_from_slice(&stream[44..44 + size]);
    write.finish().unwrap();
    let input = Input::new(buffer).unwrap();
    let access = input.prepare(0, size).unwrap();
    let packet = access
      .packet(rkmpp::Payload::Input(0..size), 0, Source::End as i64)
      .unwrap();
    decoder.packet(&packet).unwrap();
    let frame = decoder.frame().unwrap().unwrap();
    let info = frame.info();
    assert!(info.info_change);
    let mut requirements = Requirements::from_frame(info);
    requirements.minimum = 32;
    let mut output = Output::new(crate::quantize::Factory::default());
    output
      .register(
        1,
        (0..count).map(|_| Buffer::allocate(bytes).unwrap()).collect(),
        None,
      )
      .unwrap();
    let (_, reply) = output
      .push(FrameInput { frame, info, requirements }, &mut decoder)
      .unwrap();
    assert_eq!(reply.unwrap().count, 32);
    assert!(!decoder.has_output());
    for requested in [0, count, 33] {
      assert!(
        output.start(requested, true, Some(&mut decoder)).is_err()
      );
      assert!(
        !decoder.has_output(),
        "rejected reuse changed MPP output storage"
      );
      assert_eq!(output.status(), Status::Waiting);
      assert_eq!(output.id(), 1);
    }
  }
}
