//! module: syntax::hevc; responsibility: parse HEVC SPS
//! limits and geometry. operations: parse; tests: hevc_tests.
//! entities: Sequence;
use std::io;

use crate::syntax::geometry::Geometry;
pub struct Sequence {
  pub geometry: Geometry,
  pub poc_bits: usize,
  pub temporal_nested: bool,
  pub layers: Vec<crate::Limits>,
}
use crate::syntax::bits::Bits;
pub fn parse(data: &[u8]) -> io::Result<Sequence> {
  let b = &mut Bits::new(data);
  b.skip(4)?;
  let layers = b.get(3)? as usize;
  if layers > 6 {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid HEVC temporal layer count",
    ));
  }
  let temporal_nested = b.get(1)? != 0;
  if layers == 0 && !temporal_nested {
    return Err(io::Error::other(
      "single-layer HEVC SPS requires temporal nesting",
    ));
  }
  b.skip(96)?;
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
  if chroma > 3 {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid HEVC chroma format",
    ));
  }
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
  let mut limits: Vec<crate::Limits> = Vec::new();
  for _ in if all { 0 } else { layers }..=layers {
    let buffering = b.ue()?;
    let order = b.ue()?;
    let increase = b.ue()?;
    if buffering > 15 || order > buffering || increase == u32::MAX {
      return Err(io::Error::other("invalid HEVC reorder bound"));
    }
    let current = crate::Limits::new(
      buffering as usize + 1,
      order as usize,
      if increase == 0 {
        None
      } else {
        Some(u64::from(order) + u64::from(increase) - 1)
      },
    )?;
    // H.265 7.4.3.2.1 requires both budgets to be nondecreasing
    // across temporal layers. Validating each layer independently
    // would let the highest operating point underbudget the DPB.
    if limits.last().is_some_and(|previous| {
      current.capacity() < previous.capacity()
        || current.reorder() < previous.reorder()
    }) {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "HEVC temporal buffering or reorder budget decreases",
      ));
    }
    limits.push(current);
  }
  if !all {
    limits.resize(layers + 1, limits[0]);
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
  Ok(Sequence { geometry, poc_bits, temporal_nested, layers: limits })
}
