//! module: frontend_tests; responsibility: define uniform codec
//! admission. operations: test entrypoints; entities: none; tests: all
//! five codecs, metadata, task identity, budget/semantic rollback and
//! damage recovery.
use rkmpp::Payload;

use crate::frontend::Frontend;
use crate::timestamp::Source;

#[test]
fn test_admit_semantic_rejection_cannot_install_parameter_sets() {
  let packet =
    &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454];
  let codec = u32::from_le_bytes(*b"H264");
  let prefix = codec_parser::Validator::new(codec)
    .unwrap()
    .validate(packet, 1)
    .unwrap()[0]
    .prefix;
  let mut damaged = packet[..prefix].to_vec();
  // Native header validation accepts SEI framing; recovery-point
  // semantics reject this truncated Exp-Golomb payload in the service
  // admission layer.
  damaged.extend_from_slice(&[0, 0, 1, 6, 6, 1, 0, 0x80]);
  damaged.extend_from_slice(&packet[prefix..]);
  assert!(
    codec_parser::Validator::new(codec)
      .unwrap()
      .validate(&damaged, 1)
      .is_ok()
  );
  let mut frontend = Frontend::new(codec).unwrap();
  assert_eq!(
    frontend.admit(&damaged, 0, Source::End, 1).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  assert!(
    frontend.admit(&packet[prefix..], 1, Source::End, 1).is_err()
  );
  assert_eq!(
    frontend.admit(packet, 2, Source::End, 1).unwrap().tasks.len(),
    1
  );
}

#[test]
fn test_admit_annexb_budget_rejection_preserves_committed_configuration()
 {
  for (codec, packet) in [
    (
      *b"H264",
      &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454],
    ),
    (
      *b"HEVC",
      &include_bytes!("../tests/artifacts/hevc-yuv420p.hevc")[..5496],
    ),
  ] {
    let fourcc = u32::from_le_bytes(codec);
    let prefix = codec_parser::Validator::new(fourcc)
      .unwrap()
      .validate(packet, 1)
      .unwrap()[0]
      .prefix;
    let mut frontend = Frontend::new(fourcc).unwrap();
    assert!(frontend.admit(packet, 0, Source::End, 0).is_err());
    assert!(
      frontend.admit(&packet[prefix..], 1, Source::End, 1).is_err(),
      "rejected {codec:?} input installed new parameter sets"
    );
    assert_eq!(
      frontend.admit(packet, 2, Source::End, 1).unwrap().tasks.len(),
      1
    );
  }
}

#[test]
fn test_admit_annexb_metadata_does_not_require_a_picture_budget() {
  for (codec, packet) in [
    (
      *b"H264",
      &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454],
    ),
    (
      *b"HEVC",
      &include_bytes!("../tests/artifacts/hevc-yuv420p.hevc")[..5496],
    ),
  ] {
    let fourcc = u32::from_le_bytes(codec);
    let prefix = codec_parser::Validator::new(fourcc)
      .unwrap()
      .validate(packet, 1)
      .unwrap()[0]
      .prefix;
    let mut frontend = Frontend::new(fourcc).unwrap();
    assert!(
      frontend
        .admit(&packet[..prefix], 0, Source::End, 0)
        .unwrap()
        .tasks
        .is_empty()
    );
    assert_eq!(
      frontend
        .admit(&packet[prefix..], 1, Source::End, 1)
        .unwrap()
        .tasks
        .len(),
      1
    );
  }
}

#[test]
fn test_admit_repeated_damage_restores_standalone_av1_configuration() {
  let stream = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let packet = &stream[44..44 + size];
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  assert!(
    frontend
      .admit(&packet[..15], 0, Source::End, 32)
      .unwrap()
      .tasks
      .is_empty()
  );
  for _ in 0..32 {
    assert_eq!(
      frontend.admit(&[0xff], 0, Source::End, 32).err().unwrap().kind(),
      std::io::ErrorKind::InvalidData
    );
  }
  let admitted =
    frontend.admit(&packet[15..], 123, Source::Start, 1).unwrap();
  assert_eq!(admitted.tasks.len(), 1);
  let task = &admitted.tasks[0];
  assert_eq!(task.descriptor.timestamp, 123);
  assert_eq!(task.descriptor.source, Source::Start);
  let Payload::Prefixed { prefix, range } = &task.payload else {
    panic!("recovered frame needs the saved standalone configuration");
  };
  let mut restored = prefix.clone();
  restored.extend_from_slice(&packet[15..][range.clone()]);
  assert_eq!(
    codec_parser::Validator::new(u32::from_le_bytes(*b"AV01"))
      .unwrap()
      .validate(&restored, 1)
      .unwrap()
      .len(),
    1
  );
}

#[test]
fn test_admit_av1_self_configuring_recovery_keeps_input_borrowed() {
  let stream = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(stream[32..36].try_into().unwrap()) as usize;
  let packet = &stream[44..44 + size];
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  frontend.admit(packet, 0, Source::End, 32).unwrap();
  for damaged in [false, true] {
    if damaged {
      assert!(frontend.admit(&[0xff], 1, Source::End, 32).is_err());
    } else {
      frontend.seek().unwrap();
    }
    let admission = frontend.admit(packet, 2, Source::End, 32).unwrap();
    assert_eq!(admission.tasks.len(), 1);
    assert!(matches!(&admission.tasks[0].payload,
      Payload::Input(range) if *range == (0..packet.len())));
  }
}

#[test]
fn test_admit_recovery_and_seek_never_reuse_picture_identity() {
  let h264 =
    &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454];
  let hevc =
    &include_bytes!("../tests/artifacts/hevc-yuv420p.hevc")[..5496];
  let ivf = |bytes: &'static [u8]| {
    let size =
      u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    &bytes[44..44 + size]
  };
  for (codec, bytes) in [
    (*b"H264", h264),
    (*b"HEVC", hevc),
    (
      *b"VP80",
      ivf(include_bytes!("../tests/artifacts/vp8-yuv420p.ivf")),
    ),
    (
      *b"VP90",
      ivf(include_bytes!("../tests/artifacts/vp9-yuv420p.ivf")),
    ),
    (
      *b"AV01",
      ivf(include_bytes!("../tests/artifacts/av1-yuv420p.ivf")),
    ),
  ] {
    let mut frontend =
      Frontend::new(u32::from_le_bytes(codec)).unwrap();
    let first =
      frontend.admit(bytes, 0, Source::End, 32).unwrap().tasks[0]
        .descriptor
        .plan
        .id();
    assert!(frontend.admit(&[0xff], 1, Source::End, 32).is_err());
    let recovered =
      frontend.admit(bytes, 2, Source::End, 32).unwrap().tasks[0]
        .descriptor
        .plan
        .id();
    assert!(
      recovered > first,
      "recovery reused {codec:?} picture identity"
    );
    frontend.seek().unwrap();
    let sought =
      frontend.admit(bytes, 3, Source::End, 32).unwrap().tasks[0]
        .descriptor
        .plan
        .id();
    assert!(
      sought > recovered,
      "seek reused {codec:?} picture identity"
    );
  }
}

#[test]
fn test_admit_frame_budget_rejects_expansion_and_recovers_transaction()
{
  let ivf = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
  let key = &ivf[44..44 + size];
  let mut input = key.to_vec();
  for _ in 0..262_144 {
    input.extend_from_slice(&[0x1a, 1, 0x88]);
  }
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  assert_eq!(
    frontend.admit(&input, 0, Source::End, 8).err().unwrap().kind(),
    std::io::ErrorKind::InvalidData
  );
  let admitted = frontend.admit(key, 7, Source::Start, 1).unwrap();
  assert_eq!(admitted.tasks.len(), 1);
  assert_eq!(admitted.tasks[0].descriptor.timestamp, 7);
  assert!(frontend.admit(key, 8, Source::End, 0).is_err());
  assert_eq!(
    frontend.admit(key, 9, Source::End, 1).unwrap().tasks.len(),
    1
  );
}
#[test]
fn test_admit_all_codecs_carry_identity_and_submission_timestamp() {
  let h264 =
    &include_bytes!("../tests/artifacts/h264-yuv420p.h264")[..4454];
  let hevc =
    &include_bytes!("../tests/artifacts/hevc-yuv420p.hevc")[..5496];
  let vp8 = include_bytes!("../tests/artifacts/vp8-yuv420p.ivf");
  let vp9 = include_bytes!("../tests/artifacts/vp9-yuv420p.ivf");
  let av1 = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let ivf = |data: &'static [u8]| {
    &data[44..44
      + u32::from_le_bytes(data[32..36].try_into().unwrap()) as usize]
  };
  for (codec, bytes) in [
    (*b"H264", h264),
    (*b"HEVC", hevc),
    (*b"VP80", ivf(vp8)),
    (*b"VP90", ivf(vp9)),
    (*b"AV01", ivf(av1)),
  ] {
    let mut frontend =
      Frontend::new(u32::from_le_bytes(codec)).unwrap();
    let admitted =
      frontend.admit(bytes, 123, Source::Start, 32).unwrap();
    assert!(!admitted.end);
    assert_eq!(admitted.tasks.len(), 1);
    assert!(matches!(admitted.tasks[0].payload, Payload::Input(_)));
    assert_eq!(admitted.tasks[0].descriptor.timestamp, 123);
    assert_eq!(admitted.tasks[0].descriptor.source, Source::Start);
  }
}
#[test]
fn test_admit_invalid_input_cannot_create_decode_task() {
  for codec in [*b"H264", *b"HEVC", *b"VP80", *b"VP90", *b"AV01"] {
    let mut frontend =
      Frontend::new(u32::from_le_bytes(codec)).unwrap();
    assert!(frontend.admit(&[0xff], 0, Source::End, 32).is_err());
  }
}

#[test]
fn test_admit_av1_temporal_unit_assigns_each_coded_frame_its_own_task()
{
  let bytes = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  let mut offset = 32;
  let mut multiple = false;
  while offset < bytes.len() {
    let size =
      u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        as usize;
    let admission = frontend
      .admit(
        &bytes[offset + 12..offset + 12 + size],
        1,
        Source::End,
        32,
      )
      .unwrap();
    multiple |= admission.tasks.len() > 1;
    assert_eq!(
      admission
        .tasks
        .iter()
        .filter(|task| task.descriptor.plan.output().display)
        .count(),
      1
    );
    for pair in admission.tasks.windows(2) {
      assert_ne!(
        pair[0].descriptor.plan.id(),
        pair[1].descriptor.plan.id()
      );
    }
    offset += 12 + size;
  }
  assert!(
    multiple,
    "fixture includes hidden and displayed frames in one TU"
  );
}

#[test]
fn test_admit_separate_av1_sequence_and_seek_restore_decoder_configuration()
 {
  let ivf = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
  let packet = &ivf[44..44 + size];
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  assert!(
    frontend
      .admit(&packet[..15], 0, Source::End, 32)
      .unwrap()
      .tasks
      .is_empty()
  );
  let admission =
    frontend.admit(&packet[15..], 1, Source::End, 32).unwrap();
  let Payload::Prefixed { prefix, range } = &admission.tasks[0].payload
  else {
    panic!("standalone sequence must reach MPP with its picture");
  };
  assert!(!prefix.is_empty());
  assert_eq!(range, &(0..packet.len() - 15));
  frontend.seek().unwrap();
  let admission =
    frontend.admit(&packet[15..], 2, Source::End, 32).unwrap();
  assert!(matches!(
    admission.tasks[0].payload,
    Payload::Prefixed { .. }
  ));
}

#[test]
fn test_admit_oversized_metadata_picture_is_rejected_and_can_recover() {
  let ivf = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
  let packet = &ivf[44..44 + size];
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  // A legal padding OBU fills the metadata budget, but its eventual
  // picture must also fit the decoder packet contract.
  let length = 16 * 1024 * 1024 - 5;
  let mut padding = vec![0x7a];
  let mut remaining = length;
  loop {
    let mut byte = (remaining & 127) as u8;
    remaining >>= 7;
    if remaining != 0 {
      byte |= 128;
    }
    padding.push(byte);
    if remaining == 0 {
      break;
    }
  }
  padding.resize(padding.len() + length, 0);
  assert!(
    frontend
      .admit(&padding, 0, Source::End, 32)
      .unwrap()
      .tasks
      .is_empty()
  );
  let error = frontend
    .admit(packet, 1, Source::End, 32)
    .err()
    .expect("oversized combined packet");
  assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
  assert_eq!(
    frontend.admit(packet, 2, Source::End, 32).unwrap().tasks.len(),
    1
  );
}

#[test]
fn test_admit_metadata_overflow_does_not_poison_next_keyframe() {
  let ivf = include_bytes!("../tests/artifacts/av1-yuv420p.ivf");
  let size =
    u32::from_le_bytes(ivf[32..36].try_into().unwrap()) as usize;
  let packet = &ivf[44..44 + size];
  // 8 MiB legal padding OBU, admitted twice to fill the metadata
  // budget.
  let mut padding = vec![0x7a, 0xfb, 0xff, 0xff, 0x03];
  padding.resize(8 * 1024 * 1024, 0);
  let mut frontend =
    Frontend::new(u32::from_le_bytes(*b"AV01")).unwrap();
  for _ in 0..2 {
    assert!(
      frontend
        .admit(&padding, 0, Source::End, 32)
        .unwrap()
        .tasks
        .is_empty()
    );
  }
  assert!(frontend.admit(&[0x7a, 0], 1, Source::End, 32).is_err());
  assert_eq!(
    frontend.admit(packet, 2, Source::End, 32).unwrap().tasks.len(),
    1
  );
}
