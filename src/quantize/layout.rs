//! module: quantize::layout; responsibility: validate linear
//! NV15-to-NV12 conversion geometry. operations: new, parameters,
//! source_size, destination_size; entities: Layout; tests:
//! layout_tests.
use std::io;

use crate::image::Plane;
#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
  parameters: [u32; 6],
}
impl Layout {
  pub(super) fn new(
    width: u32,
    height: u32,
    source: Plane<'_>,
    destination: Plane<'_>,
  ) -> io::Result<Self> {
    if width == 0
      || height == 0
      || width > 8192
      || height > 8192
      || source.rows() < height
      || destination.rows() < height
      || source.rows() > 16384
      || destination.rows() > 16384
      || source.stride() > 65536
      || destination.stride() > 65536
      || !source.stride().is_multiple_of(4)
      || !destination.stride().is_multiple_of(4)
      || (source.stride() as u64)
        < ((((width + 1) & !1) as u64 * 10).div_ceil(32) * 4)
      || destination.stride() < ((width + 3) & !3)
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid conversion dimensions or pitch",
      ));
    }
    Ok(Self {
      parameters: [
        width,
        height,
        source.stride(),
        source.rows(),
        destination.stride(),
        destination.rows(),
      ],
    })
  }

  pub(super) fn parameters(self) -> [u32; 6] {
    self.parameters
  }

  pub(super) fn source_size(self) -> u64 {
    self.parameters[2] as u64
      * (self.parameters[3] + self.parameters[1].div_ceil(2)) as u64
  }

  pub(super) fn destination_size(self) -> u64 {
    self.parameters[4] as u64
      * (self.parameters[5] + self.parameters[1].div_ceil(2)) as u64
  }
}
