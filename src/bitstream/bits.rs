//! module: bitstream::bits; responsibility: read bounded RBSP syntax.
//! operations: new, get, skip, ue, se; entities: Bits; tests:
//! bits_tests.
use std::io;
pub(super) struct Bits {
  bytes: Vec<u8>,
  bit: usize,
}
impl Bits {
  pub(super) fn new(data: &[u8]) -> Self {
    let mut bytes = Vec::with_capacity(data.len());
    let mut zeros = 0;
    for &value in data {
      if zeros >= 2 && value == 3 {
        zeros = 0;
        continue;
      }
      bytes.push(value);
      zeros = if value == 0 { zeros + 1 } else { 0 };
    }
    Self { bytes, bit: 0 }
  }

  pub(super) fn from_rbsp(data: &[u8]) -> Self {
    Self { bytes: data.to_vec(), bit: 0 }
  }

  /// A stop bit followed only by zero padding is not syntax data.
  pub(super) fn more_rbsp_data(&self) -> bool {
    let end = self.bytes.len() * 8;
    if self.bit == end {
      return false;
    }
    let one =
      |bit: usize| self.bytes[bit / 8] & (1 << (7 - bit % 8)) != 0;
    !one(self.bit) || (self.bit + 1..end).any(one)
  }

  /// Consume rbsp_trailing_bits plus any Annex B trailing_zero_8bits
  /// retained by the NAL splitter. A missing stop bit is an error.
  pub(super) fn finish_rbsp(&mut self) -> io::Result<()> {
    if self.get(1)? != 1 {
      return Err(io::Error::other("missing RBSP stop bit"));
    }
    while self.bit < self.bytes.len() * 8 {
      if self.get(1)? != 0 {
        return Err(io::Error::other("invalid RBSP padding"));
      }
    }
    Ok(())
  }

  pub(super) fn get(&mut self, count: usize) -> io::Result<u32> {
    if count > 32 || self.bit + count > self.bytes.len() * 8 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "truncated SPS",
      ));
    }
    let mut value = 0;
    for _ in 0..count {
      value = (value << 1)
        | u32::from(
          (self.bytes[self.bit / 8] >> (7 - self.bit % 8)) & 1,
        );
      self.bit += 1;
    }
    Ok(value)
  }

  pub(super) fn skip(&mut self, count: usize) -> io::Result<()> {
    for _ in 0..count {
      self.get(1)?;
    }
    Ok(())
  }

  pub(super) fn ue(&mut self) -> io::Result<u32> {
    let mut zeros = 0;
    while self.get(1)? == 0 {
      zeros += 1;
      if zeros > 32 {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "SPS code overflow",
        ));
      }
    }
    u32::try_from((1u64 << zeros) - 1 + u64::from(self.get(zeros)?))
      .map_err(|_| io::Error::other("unsigned Exp-Golomb overflow"))
  }

  pub(super) fn ue_incremented(&mut self) -> io::Result<u32> {
    self
      .ue()?
      .checked_add(1)
      .ok_or_else(|| io::Error::other("Exp-Golomb increment overflow"))
  }

  pub(super) fn se(&mut self) -> io::Result<i32> {
    let value = self.ue()?;
    let signed = if value & 1 != 0 {
      i64::from(value).div_euclid(2) + 1
    } else {
      -i64::from(value) / 2
    };
    i32::try_from(signed)
      .map_err(|_| io::Error::other("signed Exp-Golomb overflow"))
  }
}
