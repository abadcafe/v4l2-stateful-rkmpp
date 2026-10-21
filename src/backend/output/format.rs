//! module: output::format; responsibility: validate immutable
//! decoded-to-CAPTURE geometry. operations:
//! new/message/requirements/ten/changed; entities: Format,
//! FieldArrangement; tests: format_tests, service crop/color/pixel
//! tests.
use std::io;

use rkmpp::FrameInfo;

use crate::backend::color::Color;
use crate::backend::layout::Requirements;
use crate::protocol::{Message, Reply};
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldArrangement {
  Progressive,
  InterlacedTopFirst,
  InterlacedBottomFirst,
  Alternate,
  Interlaced,
}
impl TryFrom<u32> for FieldArrangement {
  type Error = io::Error;

  fn try_from(field: u32) -> io::Result<Self> {
    match field {
      0 => Ok(Self::Progressive),
      1 => Ok(Self::InterlacedTopFirst),
      2 => Ok(Self::InterlacedBottomFirst),
      3 | 4 => Ok(Self::Alternate),
      5 => Ok(Self::Interlaced),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid picture field",
      )),
    }
  }
}
pub(super) struct Format {
  info: FrameInfo,
  arrangement: FieldArrangement,
  requirements: Requirements,
  message: Message,
}
impl Format {
  pub(super) fn new(
    info: FrameInfo,
    requirements: Requirements,
  ) -> io::Result<Self> {
    let arrangement =
      FieldArrangement::try_from(crate::backend::frame::field(info)?)?;
    if !(18..=32).contains(&requirements.minimum) {
      return Err(io::Error::other("invalid CAPTURE minimum"));
    }
    if info.format != 0 && info.format != 1 {
      return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "MPP output is not linear NV12/NV15",
      ));
    }
    let Requirements {
      coded_width,
      coded_height,
      left: crop_left,
      top: crop_top,
      ..
    } = requirements;
    if info.width == 0
      || info.height == 0
      || (info.width, info.height)
        != (requirements.width, requirements.height)
      || !(64..=8192).contains(&coded_width)
      || !(64..=8192).contains(&coded_height)
      || u64::from(crop_left) + u64::from(info.width)
        > u64::from(coded_width)
      || u64::from(crop_top) + u64::from(info.height)
        > u64::from(coded_height)
      || info.rows < coded_height
      || info.rows > 16384
      || !info.rows.is_multiple_of(2)
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid decoded picture geometry",
      ));
    }
    let ten = info.format == 1;
    let stride = if ten {
      coded_width
        .checked_add(63)
        .ok_or_else(|| io::Error::other("frame width overflow"))?
        & !63
    } else {
      info.stride
    };
    let size = if ten {
      (stride as usize)
        .checked_mul(info.rows as usize)
        .and_then(|n| n.checked_mul(3))
        .map(|n| n / 2)
        .ok_or_else(|| io::Error::other("frame size overflow"))?
    } else {
      info.size
    };
    if stride < coded_width.next_multiple_of(2)
      || stride > 65536
      || size < stride as usize * info.rows as usize * 3 / 2
      || size > 128 * 1024 * 1024
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid decoded picture storage",
      ));
    }
    let color = Color::new(
      info.primaries,
      info.matrix,
      info.transfer,
      info.range,
    );
    for (component, value) in color.unrepresented() {
      eprintln!(
        "V4L2 cannot represent CICP {component}={value}; the client \
         must preserve the stream metadata"
      );
    }
    let message = Message {
      kind: Reply::Layout as u32,
      count: if ten {
        crate::protocol::MIN_CAPTURE
      } else {
        requirements.minimum
      },
      width: info.width,
      height: info.height,
      coded_width,
      coded_height,
      crop_left,
      crop_top,
      stride,
      rows: info.rows,
      size: u32::try_from(size).map_err(|_| {
        io::Error::other("frame buffer exceeds protocol size")
      })?,
      format: 1,
      field: crate::backend::frame::field(info)?,
      colorspace: color.primaries(),
      ycbcr_enc: color.matrix(),
      xfer_func: color.transfer(),
      quantization: color.range(),
      ..Default::default()
    };
    Ok(Self { info, arrangement, requirements, message })
  }

  pub(super) fn message(&self) -> Message {
    self.message
  }

  pub(super) fn requirements(&self) -> Requirements {
    self.requirements
  }

  pub(super) fn ten(&self) -> bool {
    self.info.format == 1
  }

  pub(super) fn changed(
    &self,
    info: FrameInfo,
    requirements: Requirements,
  ) -> bool {
    self.requirements != requirements
      || crate::backend::frame::field(info)
        .and_then(FieldArrangement::try_from)
        .ok()
        != Some(self.arrangement)
      || !same_output(self.info, info)
  }
}
fn same_output(a: FrameInfo, b: FrameInfo) -> bool {
  (
    a.width,
    a.height,
    a.stride,
    a.rows,
    a.format,
    a.primaries,
    a.matrix,
    a.transfer,
    a.range,
    a.size,
  ) == (
    b.width,
    b.height,
    b.stride,
    b.rows,
    b.format,
    b.primaries,
    b.matrix,
    b.transfer,
    b.range,
    b.size,
  )
}
