//! module: h264::nal; responsibility: admit H.264 NAL header semantics.
//! operations: Header TryFrom/kind; entities: Header; tests: nal_tests
//! and h264_tests. H.264 7.4.1 constrains nal_ref_idc by unit kind.
#![forbid(unsafe_code)]
use std::io;

pub(super) struct Header(u8);
impl TryFrom<u8> for Header {
  type Error = io::Error;

  fn try_from(value: u8) -> io::Result<Self> {
    let reference = value & 0x60 != 0;
    let valid_reference = match value & 31 {
      5 | 7 | 8 | 13 | 15 => reference,
      6 | 9..=12 => !reference,
      _ => true,
    };
    if value & 0x80 != 0 || !valid_reference {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid H264 NAL reference flags",
      ));
    }
    Ok(Self(value))
  }
}
impl Header {
  pub(super) fn kind(&self) -> u8 {
    self.0 & 31
  }
}
