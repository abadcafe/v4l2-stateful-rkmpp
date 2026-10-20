//! module: bitstream::restriction; responsibility: read H264 VUI
//! reorder limits within the SPS buffering budget. operations: h264;
//! entities: none; tests: restriction_tests, h264/sequence_tests.
use std::io;

use super::bits::Bits;
fn hrd(b: &mut Bits) -> io::Result<()> {
  let count = b.ue()?;
  if count > 31 {
    return Err(io::Error::other("invalid HRD count"));
  }
  b.skip(8)?;
  for _ in 0..=count {
    b.ue()?;
    b.ue()?;
    b.skip(1)?;
  }
  b.skip(20)
}
pub(super) fn h264(
  b: &mut Bits,
  allowed_buffering: std::ops::RangeInclusive<usize>,
) -> io::Result<Option<crate::picture::Limits>> {
  if b.get(1)? != 0 && b.get(8)? == 255 {
    b.skip(32)?;
  }
  if b.get(1)? != 0 {
    b.skip(1)?;
  }
  if b.get(1)? != 0 {
    b.skip(4)?;
    if b.get(1)? != 0 {
      b.skip(24)?;
    }
  }
  if b.get(1)? != 0 {
    b.ue()?;
    b.ue()?;
  }
  if b.get(1)? != 0 {
    b.skip(65)?;
  }
  let nal = b.get(1)? != 0;
  if nal {
    hrd(b)?;
  }
  let vcl = b.get(1)? != 0;
  if vcl {
    hrd(b)?;
  }
  if nal || vcl {
    b.skip(1)?;
  }
  b.skip(1)?;
  if b.get(1)? == 0 {
    return Ok(None);
  }
  b.skip(1)?;
  for _ in 0..4 {
    b.ue()?;
  }
  let reorder = b.ue()?;
  let buffering = b.ue()?;
  if reorder > buffering
    || buffering > 16
    || !allowed_buffering.contains(&(buffering as usize))
  {
    return Err(io::Error::other("invalid H264 reorder bound"));
  }
  Ok(Some(crate::picture::Limits::new(
    buffering as usize,
    reorder as usize,
    None,
  )?))
}
