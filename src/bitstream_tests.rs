//! module: bitstream_tests; responsibility: framed Annex B semantic
//! admission, geometry, configuration and restart contracts.
//! operations: test entrypoints; entities: none; tests:
//! test_admit_hevc_temporal_header_violations_reject_and_recover,
//! test_admit_preserves_native_allocation_failure_after_recovery,
//! test_admit_complete_picture_produces_borrowed_view_and_geometry,
//! test_admit_damage_is_reported_before_any_decode_task,
//! test_admit_metadata_retains_configuration_and_seek_requires_resume_point,
//! test_admit_natural_boundary_retains_parameters_for_next_sequence,
//! test_admit_metadata_only_sei_is_retained_for_next_coded_picture,
//! test_admit_combined_sequence_and_stream_end_preserves_picture_and_terminal,
//! test_admit_separate_metadata_is_not_duplicated_in_decoder_packet,
//! test_seek_repeated_restart_keeps_configuration_byte_identical,
//! test_seek_self_contained_picture_keeps_its_original_input_view.
//! test_admit_disposable_frame_number_reuse_preserves_every_picture.
use rkmpp::Payload;

use crate::bitstream::Parser;
#[test]
fn test_admit_disposable_frame_number_reuse_preserves_every_picture() {
  let bytes =
    include_bytes!("../tests/artifacts/h264-disposable-frame-num.h264");
  let mut parser = Parser::new(u32::from_le_bytes(*b"H264")).unwrap();
  let mut offset = 0;
  let mut identities = std::collections::BTreeSet::new();
  let mut orders = Vec::new();
  for size in [
    2724, 873, 346, 252, 254, 758, 331, 230, 222, 747, 391, 216, 264,
    521, 329, 313,
  ] {
    let report =
      parser.admit(&bytes[offset..offset + size], 1).unwrap();
    assert_eq!(report.units.len(), 1);
    let plan = &report.units[0].plan;
    assert!(identities.insert(plan.id()));
    if !plan.reference() {
      assert!(!plan.references().contains(&plan.id()));
    }
    orders.push(plan.output().order);
    offset += size;
  }
  assert_eq!(offset, bytes.len());
  orders.sort_unstable();
  assert_eq!(orders, (0..32).step_by(2).collect::<Vec<_>>());
}
fn samples() -> [([u8; 4], &'static [u8], usize, usize); 2] {
  [
    (
      *b"H264",
      include_bytes!("../tests/artifacts/h264-yuv420p.h264"),
      4454,
      6728,
    ),
    (
      *b"HEVC",
      include_bytes!("../tests/artifacts/hevc-yuv420p.hevc"),
      5496,
      7063,
    ),
  ]
}

#[test]
fn test_admit_hevc_temporal_header_violations_reject_and_recover() {
  let bytes = include_bytes!("../tests/artifacts/hevc-yuv420p.hevc");
  for kind in [3u8, 32, 33] {
    for temporal_id in [1, 6] {
      let mut parser =
        Parser::new(u32::from_le_bytes(*b"HEVC")).unwrap();
      assert_eq!(
        parser.admit(&bytes[..5496], 1).unwrap().units.len(),
        1
      );
      let (source, original) = if kind == 3 {
        (&bytes[5496..7063], 1)
      } else {
        (&bytes[..5496], kind)
      };
      let mut bad = source.to_vec();
      let offset = bad
        .windows(4)
        .position(|data| data == [0, 0, 1, original << 1])
        .unwrap()
        + 3;
      assert_eq!(bad[offset + 1], 1);
      bad[offset] = kind << 1;
      bad[offset + 1] = temporal_id + 1;
      let result = parser.admit(&bad, 1);
      assert!(result.is_err());
      assert_eq!(
        result.err().unwrap().kind(),
        std::io::ErrorKind::InvalidData
      );
      assert_eq!(
        parser.admit(&bytes[..5496], 1).unwrap().units.len(),
        1
      );
      assert_eq!(
        parser.admit(&bytes[5496..7063], 1).unwrap().units.len(),
        1
      );
    }
  }
}

#[test]
fn test_admit_preserves_native_allocation_failure_after_recovery() {
  const MARKER: &str = "RKMPP_TEST_PARSER_ALLOCATION_CHILD";
  if std::env::var_os(MARKER).is_some() {
    for (codec, bytes, first, _) in samples() {
      let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
      let error = parser.admit(&[0xff], 1).unwrap_err();
      assert_eq!(error.kind(), std::io::ErrorKind::OutOfMemory);
      assert_eq!(
        parser.admit(&bytes[..first], 1).unwrap().units.len(),
        1
      );
    }
    return;
  }
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let directory = std::env::temp_dir().join(format!(
    "rkmpp-parser-allocation-{}-{nonce}",
    std::process::id()
  ));
  std::fs::create_dir(&directory).unwrap();
  let source = directory.join("allocation.rs");
  let library = directory.join("libparser_allocation.so");
  // Only this child's second native construction fails. Other calls
  // use the installed SDK, including the following successful reset.
  std::fs::write(
    &source,
    r#"
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
#[link(name = "dl")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
macro_rules! constructor {
    ($name:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() -> *mut c_void {
            static CALLS: AtomicUsize = AtomicUsize::new(0);
            if CALLS.fetch_add(1, Ordering::SeqCst) == 1 {
                return std::ptr::null_mut();
            }
            // SAFETY: RTLD_NEXT selects the linked SDK's same C ABI.
            let symbol = unsafe {
                dlsym((-1isize) as *mut c_void,
                    concat!(stringify!($name), "\0").as_ptr().cast())
            };
            assert!(!symbol.is_null());
            // SAFETY: both exported constructors take no arguments
            // and return one owned native parser pointer.
            let original: unsafe extern "C" fn() -> *mut c_void =
                unsafe { std::mem::transmute(symbol) };
            unsafe { original() }
        }
    };
}
constructor!(gst_h264_nal_parser_new);
constructor!(gst_h265_parser_new);
"#,
  )
  .unwrap();
  let compiled = std::process::Command::new("rustc")
    .args(["--edition=2024", "--crate-type=cdylib", "-Dwarnings"])
    .arg(&source)
    .arg("-o")
    .arg(&library)
    .output()
    .unwrap();
  assert!(
    compiled.status.success(),
    "{}",
    String::from_utf8_lossy(&compiled.stderr)
  );
  let observed = std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "bitstream_tests::test_admit_preserves_native_allocation_failure_after_recovery", "--nocapture"])
    .env(MARKER, "1")
    .env("LD_PRELOAD", &library)
    .output().unwrap();
  std::fs::remove_dir_all(directory).unwrap();
  assert!(
    observed.status.success(),
    "{}{}",
    String::from_utf8_lossy(&observed.stdout),
    String::from_utf8_lossy(&observed.stderr)
  );
}
#[test]
fn test_admit_complete_picture_produces_borrowed_view_and_geometry() {
  for (codec, data, first, second) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    for bytes in [&data[..first], &data[first..second]] {
      let report = parser.admit(bytes, 1).unwrap();
      assert_eq!(report.units.len(), 1);
      let unit = &report.units[0];
      let Payload::Input(range) = &unit.payload else {
        panic!("ordinary AU must borrow its input");
      };
      assert_eq!(range, &(0..bytes.len()));
      let g = unit.geometry;
      assert_eq!(
        (g.width, g.height, g.coded_width, g.coded_height),
        (256, 128, 256, 128)
      );
      assert!(!report.end);
    }
  }
}
#[test]
fn test_admit_damage_is_reported_before_any_decode_task() {
  for (codec, _, _, _) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    assert_eq!(
      parser.admit(&[0, 0, 1, 0xff], 1).unwrap_err().kind(),
      std::io::ErrorKind::InvalidData
    );
  }
}
#[test]
fn test_admit_metadata_retains_configuration_and_seek_requires_resume_point()
 {
  for (codec, data, first, second) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    let start = data[..first]
      .windows(3)
      .enumerate()
      .find_map(|(i, w)| {
        if w != [0, 0, 1] {
          return None;
        }
        let kind = if codec == *b"H264" {
          data[i + 3] & 31
        } else {
          (data[i + 3] >> 1) & 63
        };
        (if codec == *b"H264" { kind == 5 } else { kind <= 31 })
          .then_some(i)
      })
      .unwrap();
    let metadata = parser.admit(&data[..start], 1).unwrap();
    assert!(metadata.units.is_empty() && !metadata.end);
    assert_eq!(
      parser.admit(&data[start..first], 1).unwrap().units.len(),
      1
    );
    parser.seek().unwrap();
    assert!(
      parser.admit(&data[first..second], 1).unwrap().units.is_empty()
    );
    let resumed = parser.admit(&data[start..first], 1).unwrap();
    assert_eq!(resumed.units.len(), 1);
    let Payload::Prefixed { prefix, range } = &resumed.units[0].payload
    else {
      panic!("restart must restore configuration");
    };
    assert!(!prefix.is_empty());
    assert_eq!(range, &(0..first - start));
  }
}
#[test]
fn test_admit_natural_boundary_retains_parameters_for_next_sequence() {
  for (codec, data, first, _) in samples() {
    for (kind, end) in [(0, false), (1, true)] {
      let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
      parser.admit(&data[..first], 1).unwrap();
      let marker = if codec == *b"H264" {
        vec![0, 0, 1, 10 + kind]
      } else {
        vec![0, 0, 1, 72 + kind * 2, 1]
      };
      let report = parser.admit(&marker, 1).unwrap();
      assert!(report.units.is_empty());
      assert_eq!(report.end, end);
      assert_eq!(
        parser.admit(&data[..first], 1).unwrap().units.len(),
        1
      );
    }
  }
}

#[test]
fn test_admit_metadata_only_sei_is_retained_for_next_coded_picture() {
  for (codec, data, first, second) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    parser.admit(&data[..first], 1).unwrap();
    let mut sei = if codec == *b"H264" {
      vec![0, 0, 1, 6]
    } else {
      vec![0, 0, 1, 78, 1]
    };
    sei.extend([5, 16]);
    sei.extend(0u8..16);
    sei.push(0x80);
    assert!(parser.admit(&sei, 1).unwrap().units.is_empty());
    let report = parser.admit(&data[first..second], 1).unwrap();
    let Payload::Prefixed { prefix, range } = &report.units[0].payload
    else {
      panic!("metadata must be retained with its next picture");
    };
    assert_eq!(
      prefix.windows(sei.len()).filter(|window| *window == sei).count(),
      1
    );
    assert_eq!(range, &(0..second - first));
  }
}

#[test]
fn test_admit_combined_sequence_and_stream_end_preserves_picture_and_terminal()
 {
  for (codec, data, first, _) in samples() {
    let markers = if codec == *b"H264" {
      vec![vec![0, 0, 1, 10], vec![0, 0, 1, 11]]
    } else {
      vec![vec![0, 0, 1, 72, 1], vec![0, 0, 1, 74, 1]]
    };
    for picture in [false, true] {
      let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
      let mut packet =
        if picture { data[..first].to_vec() } else { Vec::new() };
      packet.extend_from_slice(&markers.concat());
      let report = parser.admit(&packet, 1).unwrap();
      assert!(report.end);
      assert_eq!(report.units.len(), usize::from(picture));
      if picture {
        assert!(
          matches!(&report.units[0].payload, Payload::Input(range) if range == &(0..first))
        );
      }
    }
    for tail in [vec![1, 0], vec![0, 0], vec![1, 1]] {
      let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
      let bytes: Vec<_> =
        tail.into_iter().flat_map(|i| markers[i].clone()).collect();
      assert!(parser.admit(&bytes, 1).is_err());
    }
  }
}

#[test]
fn test_admit_separate_metadata_is_not_duplicated_in_decoder_packet() {
  for (codec, data, first, _) in samples() {
    let split = data[..first]
      .windows(3)
      .enumerate()
      .find_map(|(i, w)| {
        if w != [0, 0, 1] {
          return None;
        }
        let picture = if codec == *b"H264" {
          data[i + 3] & 31 == 5
        } else {
          (data[i + 3] >> 1) & 63 <= 31
        };
        picture.then_some(i)
      })
      .unwrap();
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    assert!(parser.admit(&data[..split], 1).unwrap().units.is_empty());
    let report = parser.admit(&data[split..first], 1).unwrap();
    let Payload::Prefixed { prefix, range } = &report.units[0].payload
    else {
      panic!("separate metadata needs one assembled packet");
    };
    assert_eq!(prefix, &data[..split]);
    assert_eq!(range, &(0..first - split));
  }
}

#[test]
fn test_seek_repeated_restart_keeps_configuration_byte_identical() {
  for (codec, data, first, _) in samples() {
    let split = data[..first]
      .windows(3)
      .enumerate()
      .find_map(|(i, w)| {
        let picture = w == [0, 0, 1]
          && if codec == *b"H264" {
            data[i + 3] & 31 == 5
          } else {
            (data[i + 3] >> 1) & 63 <= 31
          };
        picture.then_some(i)
      })
      .unwrap();
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    parser.admit(&data[..first], 1).unwrap();
    let mut previous = None;
    for _ in 0..64 {
      parser.seek().unwrap();
      let report = parser.admit(&data[split..first], 1).unwrap();
      let Payload::Prefixed { prefix, range } =
        &report.units[0].payload
      else {
        panic!("restart must restore configuration");
      };
      assert_eq!(range, &(0..first - split));
      if let Some(previous) = &previous {
        assert_eq!(prefix, previous, "configuration grew after seek");
      }
      previous = Some(prefix.clone());
    }
  }
}

#[test]
fn test_seek_self_contained_picture_keeps_its_original_input_view() {
  for (codec, data, first, _) in samples() {
    let bytes = &data[..first];
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    parser.admit(bytes, 1).unwrap();
    for _ in 0..8 {
      parser.seek().unwrap();
      let report = parser.admit(bytes, 1).unwrap();
      assert_eq!(report.units.len(), 1);
      assert!(matches!(
        &report.units[0].payload,
        Payload::Input(range) if range == &(0..bytes.len())
      ));
    }
  }
}

#[test]
fn test_seek_separate_metadata_restores_each_parameter_once() {
  for (codec, data, first, _) in samples() {
    let split = data[..first]
      .windows(3)
      .enumerate()
      .find_map(|(offset, start)| {
        (start == [0, 0, 1]
          && if codec == *b"H264" {
            data[offset + 3] & 31 == 5
          } else {
            (data[offset + 3] >> 1) & 63 <= 31
          })
        .then_some(offset)
      })
      .unwrap();
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    parser.admit(&data[..first], 1).unwrap();
    parser.seek().unwrap();
    assert!(parser.admit(&data[..split], 1).unwrap().units.is_empty());
    let report = parser.admit(&data[split..first], 1).unwrap();
    let Payload::Prefixed { prefix, range } = &report.units[0].payload
    else {
      panic!("separate metadata must precede the borrowed picture");
    };
    assert_eq!(prefix, &data[..split]);
    assert_eq!(range, &(0..first - split));
  }
}
