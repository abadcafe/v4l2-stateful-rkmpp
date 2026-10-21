//! module: syntax::h264; responsibility: parse H264 SPS
//! limits and geometry. operations: parse; entities: Sequence;
//! tests: h264_tests.
use std::io;

use crate::syntax::bits::Bits;
use crate::syntax::geometry::Geometry;
#[derive(Clone)]
pub enum Poc {
  Lsb(usize),
  Delta { nonreference: i32, bottom: i32, offsets: Vec<i32> },
  FrameNumber,
}

#[derive(Clone)]
pub struct Sequence {
  pub geometry: Geometry,
  pub frame_bits: usize,
  pub poc: Poc,
  pub maximum: u32,
  pub gaps_allowed: bool,
}
pub fn parse(data: &[u8]) -> io::Result<Sequence> {
  let b = &mut Bits::new(data);
  let profile = b.get(8)?;
  let constraints = b.get(8)?;
  let level = b.get(8)?;
  let id = b.ue()?;
  if id > 31 {
    return Err(io::Error::other("invalid H264 SPS id"));
  }
  let mut chroma = 1;
  let mut separate = false;
  if matches!(
    profile,
    100
      | 110
      | 122
      | 244
      | 44
      | 83
      | 86
      | 118
      | 128
      | 138
      | 139
      | 134
      | 135
  ) {
    chroma = b.ue()?;
    separate = chroma == 3 && b.get(1)? != 0;
    b.ue()?;
    b.ue()?;
    b.skip(1)?;
    if b.get(1)? != 0 {
      for index in 0..if chroma == 3 { 12 } else { 8 } {
        if b.get(1)? != 0 {
          let mut last = 8;
          let mut next = 8;
          for _ in 0..if index < 6 { 16 } else { 64 } {
            if next != 0 {
              next = (i64::from(last) + i64::from(b.se()?))
                .rem_euclid(256) as i32;
            }
            if next != 0 {
              last = next;
            }
          }
        }
      }
    }
  }
  let frame_bits = b.ue()? as usize + 4;
  let poc = match b.ue()? {
    0 => {
      let bits = b.ue()? as usize + 4;
      Poc::Lsb(bits)
    }
    1 => {
      b.skip(1)?;
      let nonreference = b.se()?;
      let bottom = b.se()?;
      let count = b.ue()?;
      if count > 255 {
        return Err(io::Error::other("invalid SPS cycle"));
      }
      let offsets =
        (0..count).map(|_| b.se()).collect::<io::Result<Vec<_>>>()?;
      Poc::Delta { nonreference, bottom, offsets }
    }
    2 => Poc::FrameNumber,
    _ => return Err(io::Error::other("invalid SPS POC")),
  };
  let maximum = b.ue()?;
  let gaps_allowed = b.get(1)? != 0;
  if maximum > 16
    || frame_bits > 16
    || chroma > 3
    || matches!(poc, Poc::Lsb(n) if n > 16)
  {
    return Err(io::Error::other(
      "invalid H264 SPS reference parameters",
    ));
  }
  let mb_width = b.ue_incremented()?;
  let map_height = b.ue_incremented()?;
  let frame = b.get(1)?;
  if frame == 0 {
    b.skip(1)?;
  }
  b.skip(1)?;
  let (left, right, top, bottom) = if b.get(1)? != 0 {
    let left = b.ue()?;
    let right = b.ue()?;
    let top = b.ue()?;
    let bottom = b.ue()?;
    (left, right, top, bottom)
  } else {
    (0, 0, 0, 0)
  };
  let max_mbs = match level {
    9 | 10 => 396,
    11 => {
      if constraints & 16 != 0 {
        396
      } else {
        900
      }
    }
    12 | 13 | 20 => 2376,
    21 => 4752,
    22 | 30 => 8100,
    31 => 18000,
    32 => 20480,
    40 | 41 => 32768,
    42 => 34816,
    50 => 110400,
    51 | 52 => 184320,
    60..=62 => 696320,
    _ => return Err(io::Error::other("invalid H264 level")),
  };
  let mbs = mb_width
    .checked_mul(map_height)
    .and_then(|n| n.checked_mul(2 - frame))
    .ok_or_else(|| io::Error::other("SPS dimensions overflow"))?;
  let capacity = (max_mbs / mbs).min(16) as usize;
  if capacity == 0 || maximum as usize > capacity {
    return Err(io::Error::other(
      "H264 references exceed level DPB capacity",
    ));
  }
  let signaled = if b.get(1)? != 0 {
    // H.264 E.2.1 / A.3.1: validate the signaled value before
    // normalizing zero to one working display surface. The reference
    // requirement and level budget belong to this same SPS.
    crate::syntax::restriction::h264(b, maximum as usize..=capacity)?
  } else {
    None
  };
  let limits = if let Some(limits) = signaled {
    limits
  } else {
    // E.2.1 infers both buffering and reorder, not just reorder.
    // Keep the zero reference requirement until after validation;
    // Limits then supplies the one working surface needed for output.
    let inferred = if constraints & 16 != 0
      && matches!(profile, 44 | 86 | 100 | 110 | 122 | 244)
    {
      0
    } else {
      capacity
    };
    if maximum as usize > inferred {
      return Err(io::Error::other(
        "H264 references exceed inferred buffering",
      ));
    }
    crate::Limits::new(inferred, inferred, None)?
  };
  let chroma = if separate { 0 } else { chroma };
  let x = if matches!(chroma, 1 | 2) { 2 } else { 1 };
  let y = (2 - frame) * if chroma == 1 { 2 } else { 1 };
  let coded_width = mb_width
    .checked_mul(16)
    .ok_or_else(|| io::Error::other("SPS width overflow"))?;
  let coded_height = map_height
    .checked_mul(16 * (2 - frame))
    .ok_or_else(|| io::Error::other("SPS height overflow"))?;
  let width = u32::try_from(
    i64::from(coded_width)
      - (i64::from(left) + i64::from(right)) * i64::from(x),
  )
  .map_err(|_| io::Error::other("invalid SPS crop"))?;
  let height = u32::try_from(
    i64::from(coded_height)
      - (i64::from(top) + i64::from(bottom)) * i64::from(y),
  )
  .map_err(|_| io::Error::other("invalid SPS crop"))?;
  if width == 0
    || height == 0
    || coded_width > 8192
    || coded_height > 8192
  {
    return Err(io::Error::other("invalid SPS dimensions"));
  }
  Ok(Sequence {
    frame_bits,
    poc,
    maximum,
    gaps_allowed,
    geometry: Geometry {
      id,
      width,
      height,
      coded_width,
      coded_height,
      limits,
      left: left.checked_mul(x).ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidData,
          "SPS horizontal crop overflow",
        )
      })?,
      top: top.checked_mul(y).ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidData,
          "SPS vertical crop overflow",
        )
      })?,
    },
  })
}
