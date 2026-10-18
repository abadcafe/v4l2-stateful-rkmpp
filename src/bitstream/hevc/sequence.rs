//! module: bitstream::hevc::sequence; responsibility: parse HEVC SPS
//! limits and geometry. operations: parse; tests: sequence_tests.
use std::io;

use crate::bitstream::geometry::Geometry;
use crate::bitstream::hevc_rps;
pub(in crate::bitstream) struct Sequence {
  pub geometry: Geometry,
  pub poc_bits: usize,
  pub layers: Vec<crate::picture::Limits>,
}
use crate::bitstream::bits::Bits;
pub(in crate::bitstream) fn parse(data: &[u8]) -> io::Result<Sequence> {
  let b = &mut Bits::new(data);
  b.skip(4)?;
  let layers = b.get(3)? as usize;
  b.skip(1 + 96)?;
  let mut sub = Vec::new();
  for _ in 0..layers {
    sub.push((b.get(1)?, b.get(1)?));
  }
  if layers > 0 {
    b.skip((8 - layers) * 2)?;
  }
  for (profile, level) in sub {
    if profile != 0 {
      b.skip(88)?;
    }
    if level != 0 {
      b.skip(8)?;
    }
  }
  let id = b.ue()?;
  if id > 15 {
    return Err(io::Error::other("invalid HEVC SPS id"));
  }
  let chroma = b.ue()?;
  let separate = chroma == 3 && b.get(1)? != 0;
  let coded_width = b.ue()?;
  let coded_height = b.ue()?;
  let (left, right, top, bottom) = if b.get(1)? != 0 {
    let left = b.ue()?;
    let right = b.ue()?;
    let top = b.ue()?;
    let bottom = b.ue()?;
    (left, right, top, bottom)
  } else {
    (0, 0, 0, 0)
  };
  b.ue()?;
  b.ue()?;
  let poc_bits = b.ue()? as usize + 4;
  if poc_bits > 16 {
    return Err(io::Error::other("invalid HEVC POC width"));
  }
  let all = b.get(1)? != 0;
  let mut limits = Vec::new();
  for _ in if all { 0 } else { layers }..=layers {
    let buffering = b.ue()?;
    let order = b.ue()?;
    let increase = b.ue()?;
    if buffering > 15 || order > buffering || increase == u32::MAX {
      return Err(io::Error::other("invalid HEVC reorder bound"));
    }
    limits.push(crate::picture::Limits::new(
      buffering as usize + 1,
      order as usize,
      if increase == 0 {
        None
      } else {
        Some(u64::from(order) + u64::from(increase) - 1)
      },
    )?);
  }
  if !all {
    limits.resize(layers + 1, limits[0]);
  }
  for _ in 0..6 {
    b.ue()?;
  }
  if b.get(1)? != 0 && b.get(1)? != 0 {
    for size in 0..4 {
      for _ in (0..6).step_by(if size == 3 { 3 } else { 1 }) {
        if b.get(1)? == 0 {
          b.ue()?;
        } else {
          if size > 1 {
            b.se()?;
          }
          for _ in 0..(1usize << (4 + size * 2)).min(64) {
            b.se()?;
          }
        }
      }
    }
  }
  b.skip(2)?;
  if b.get(1)? != 0 {
    b.skip(8)?;
    b.ue()?;
    b.ue()?;
    b.skip(1)?;
  }
  let count = b.ue()?;
  if count > 64 {
    return Err(io::Error::other("too many HEVC reference sets"));
  }
  let mut sets = Vec::new();
  for _ in 0..count {
    sets.push(hevc_rps::parse(b, &sets, false)?);
  }
  if b.get(1)? != 0 {
    let count = b.ue()?;
    if count > 32 {
      return Err(io::Error::other(
        "too many HEVC long-term references",
      ));
    }
    for _ in 0..count {
      b.get(poc_bits)?;
      b.skip(1)?;
    }
  }
  let x = if !separate && matches!(chroma, 1 | 2) { 2 } else { 1 };
  let y = if !separate && chroma == 1 { 2 } else { 1 };
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
  let geometry = Geometry {
    id,
    width,
    height,
    coded_width,
    coded_height,
    limits: *limits.last().unwrap(),
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
  };
  Ok(Sequence { geometry, poc_bits, layers: limits })
}
