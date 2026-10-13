//! module: mpp::decoder_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_new_all_supported_codecs_have_nonblocking_empty_frame_and_reset
//! test_new_rejects_unknown_codec
//! test_import_capture_and_ready_require_output_group
use super::decoder::Decoder;
use crate::dma::Buffer;
use crate::notify::Notify;

#[test]
fn test_new_all_supported_codecs_have_nonblocking_empty_frame_and_reset()
 {
  for codec in [*b"H264", *b"HEVC", *b"VP80", *b"VP90", *b"AV01"] {
    let mut decoder =
      Decoder::new(u32::from_le_bytes(codec), Notify::new().unwrap())
        .unwrap();
    assert!(decoder.frame().unwrap().is_none());
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
    decoder.import_capture(&[Buffer::allocate(4096).unwrap()]).is_err()
  );
  assert!(decoder.ready().is_err());
  decoder.configure_output(false).unwrap();
  assert!(decoder.has_output());
  decoder.import_capture(&[Buffer::allocate(4096).unwrap()]).unwrap();
  decoder.configure_output(true).unwrap();
  assert!(decoder.has_output());
  decoder.reset().unwrap();
  decoder.close().unwrap();
}
