//! module: backend::frame; responsibility: retain decoded image
//! ownership through conversion. operations: FrameInput implementing
//! converter::Input; entities: FrameInput; tests: output_tests,
//! converter_tests, service pixel acceptance.
use super::layout::Extent;
use crate::converter::Input;
use crate::image::{Depth, Plane};
use crate::mpp::{Frame, FrameInfo};
pub(super) struct FrameInput {
  pub frame: Frame,
  pub info: FrameInfo,
  pub extent: Extent,
}
impl Input for FrameInput {
  fn depth(&self) -> Depth {
    if self.info.format == 1 { Depth::Ten } else { Depth::Eight }
  }

  fn plane(&self) -> Plane<'_> {
    self
      .frame
      .plane()
      .expect("validated frame DMA-BUF")
      .expect("decoded frame has a buffer")
  }
}
