//! module: backend::frame; responsibility: retain decoded image
//! ownership through conversion. operations: FrameInput; entities:
//! FrameInput; tests: output_tests, service pixel acceptance.
use rkmpp::{Frame, FrameInfo};

use super::layout::Requirements;
pub(super) struct FrameInput {
  pub frame: Frame,
  pub info: FrameInfo,
  pub requirements: Requirements,
}

/// Translate supported SDK field modes at the bridge boundary.
pub(super) fn field(info: FrameInfo) -> std::io::Result<u32> {
  match info.mode & 15 {
    0 => Ok(0),
    1 => Ok(3),
    2 => Ok(4),
    7 => Ok(1),
    11 => Ok(2),
    3 | 15 => Ok(5),
    _ => Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      "unsupported MPP field mode",
    )),
  }
}
