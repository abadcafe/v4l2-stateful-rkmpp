//! module: bitstream::hevc::poc; responsibility: derive HEVC picture
//! order (H.265 8.3.1). operations: Counter::derive; entities: Counter,
//! Picture; tests: poc_tests.
use std::io;
pub(super) struct Picture {
  pub lsb: u32,
  pub bits: usize,
  pub kind: u8,
  pub temporal_id: u8,
  pub random_access: bool,
}
#[derive(Default)]
pub(super) struct Counter {
  msb: i64,
  lsb: i64,
}
impl Counter {
  pub(super) fn derive(&mut self, p: Picture) -> io::Result<i32> {
    if !(4..=16).contains(&p.bits)
      || p.lsb >= 1u32 << p.bits
      || p.temporal_id > 6
      || p.kind > 31
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid HEVC POC parameters",
      ));
    }
    let lsb = i64::from(p.lsb);
    let modulus = 1i64 << p.bits;
    let msb = if p.random_access {
      0
    } else {
      self.msb
        + if lsb < self.lsb && self.lsb - lsb >= modulus / 2 {
          modulus
        } else if lsb > self.lsb && lsb - self.lsb > modulus / 2 {
          -modulus
        } else {
          0
        }
    };
    let result = i32::try_from(msb + lsb).map_err(|_| {
      io::Error::new(io::ErrorKind::InvalidData, "HEVC POC overflow")
    })?;
    if p.temporal_id == 0 && !matches!(p.kind, 0 | 2 | 4 | 6..=9) {
      self.msb = msb;
      self.lsb = lsb;
    }
    Ok(result)
  }
}
