//! module: bitstream_tests; responsibility: framed Annex B semantic
//! admission, geometry, configuration and restart contracts.
use rkmpp::Payload;

use crate::bitstream::Parser;
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
fn test_admit_complete_picture_produces_borrowed_view_and_geometry() {
  for (codec, data, first, second) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    for bytes in [&data[..first], &data[first..second]] {
      let report = parser.admit(bytes).unwrap();
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
      parser.admit(&[0, 0, 1, 0xff]).unwrap_err().kind(),
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
    let metadata = parser.admit(&data[..start]).unwrap();
    assert!(metadata.units.is_empty() && !metadata.end);
    assert_eq!(
      parser.admit(&data[start..first]).unwrap().units.len(),
      1
    );
    parser.seek().unwrap();
    assert!(
      parser.admit(&data[first..second]).unwrap().units.is_empty()
    );
    let resumed = parser.admit(&data[start..first]).unwrap();
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
      parser.admit(&data[..first]).unwrap();
      let marker = if codec == *b"H264" {
        vec![0, 0, 1, 10 + kind]
      } else {
        vec![0, 0, 1, 72 + kind * 2, 1]
      };
      let report = parser.admit(&marker).unwrap();
      assert!(report.units.is_empty());
      assert_eq!(report.end, end);
      assert_eq!(parser.admit(&data[..first]).unwrap().units.len(), 1);
    }
  }
}

#[test]
fn test_admit_metadata_only_sei_is_retained_for_next_coded_picture() {
  for (codec, data, first, second) in samples() {
    let mut parser = Parser::new(u32::from_le_bytes(codec)).unwrap();
    parser.admit(&data[..first]).unwrap();
    let mut sei = if codec == *b"H264" {
      vec![0, 0, 1, 6]
    } else {
      vec![0, 0, 1, 78, 1]
    };
    sei.extend([5, 16]);
    sei.extend(0u8..16);
    sei.push(0x80);
    assert!(parser.admit(&sei).unwrap().units.is_empty());
    let report = parser.admit(&data[first..second]).unwrap();
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
      let report = parser.admit(&packet).unwrap();
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
      assert!(parser.admit(&bytes).is_err());
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
    assert!(parser.admit(&data[..split]).unwrap().units.is_empty());
    let report = parser.admit(&data[split..first]).unwrap();
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
    parser.admit(&data[..first]).unwrap();
    let mut previous = None;
    for _ in 0..64 {
      parser.seek().unwrap();
      let report = parser.admit(&data[split..first]).unwrap();
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
    parser.admit(bytes).unwrap();
    for _ in 0..8 {
      parser.seek().unwrap();
      let report = parser.admit(bytes).unwrap();
      assert_eq!(report.units.len(), 1);
      assert!(matches!(
        &report.units[0].payload,
        Payload::Input(range) if range == &(0..bytes.len())
      ));
    }
  }
}
