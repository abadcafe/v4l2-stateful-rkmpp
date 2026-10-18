//! module: backend::frame; responsibility: retain decoded image
//! ownership through conversion. operations: FrameInput; entities:
//! FrameInput; tests: output_tests, service pixel acceptance.
use super::layout::Requirements;
use crate::mpp::{Frame, FrameInfo};
pub(super) struct FrameInput {
  pub frame: Frame,
  pub info: FrameInfo,
  pub requirements: Requirements,
}
