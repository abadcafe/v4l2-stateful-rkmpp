//! module: syntax::bits; responsibility: read bounded RBSP syntax.
//! operations: new, get, skip, take_bytes, ue, se; entities: Bits;
//! tests: bits_tests.
use std::io;
#[derive(Clone)]
pub struct Bits<'a> {
  bytes: &'a [u8],
  zeros: Option<u8>,
  cache: u64,
  available: usize,
}
impl<'a> Bits<'a> {
  pub fn new(data: &'a [u8]) -> Self {
    let mut bits = Self::from_rbsp(data);
    bits.zeros = Some(0);
    bits
  }

  pub fn from_rbsp(data: &'a [u8]) -> Self {
    Self { bytes: data, zeros: None, cache: 0, available: 0 }
  }

  /// Split a byte-aligned payload without allocating or losing its
  /// preceding emulation-prevention context. Failure consumes nothing.
  pub fn take_bytes(&mut self, count: usize) -> io::Result<Self> {
    if self.available != 0 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unaligned RBSP payload",
      ));
    }
    let length = count.checked_mul(8).ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidData,
        "RBSP payload overflow",
      )
    })?;
    let mut end = self.clone();
    end.skip(length)?;
    let mut payload = self.clone();
    payload.bytes = &self.bytes[..self.bytes.len() - end.bytes.len()];
    *self = end;
    Ok(payload)
  }

  fn byte(&mut self) -> io::Result<Option<u8>> {
    loop {
      let Some((&value, rest)) = self.bytes.split_first() else {
        return Ok(None);
      };
      self.bytes = rest;
      if let Some(zeros) = &mut self.zeros {
        if *zeros >= 2 && value == 3 {
          // H.264/H.265 7.4.1: 00 00 03 may only precede 00..03.
          // A final prevention byte is permitted after cabac_zero_word.
          if rest.first().is_some_and(|&next| next > 3) {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "invalid emulation prevention byte",
            ));
          }
          *zeros = 0;
          continue;
        }
        *zeros = if value == 0 { zeros.saturating_add(1) } else { 0 };
      }
      return Ok(Some(value));
    }
  }

  pub fn more_rbsp_data(&self) -> bool {
    let mut remaining = self.clone();
    match remaining.get(1) {
      Err(_) => false,
      Ok(0) => true,
      Ok(_) => {
        if remaining.cache != 0 {
          return true;
        }
        loop {
          match remaining.byte() {
            Ok(Some(0)) => {}
            Ok(None) => return false,
            Ok(Some(_)) | Err(_) => return true,
          }
        }
      }
    }
  }

  pub fn finish_rbsp(&mut self) -> io::Result<()> {
    if self.get(1)? != 1 {
      return Err(io::Error::other("missing RBSP stop bit"));
    }
    if self.cache != 0 {
      return Err(io::Error::other("invalid RBSP padding"));
    }
    self.available = 0;
    while let Some(byte) = self.byte()? {
      if byte != 0 {
        return Err(io::Error::other("invalid RBSP padding"));
      }
    }
    Ok(())
  }

  pub fn get(&mut self, count: usize) -> io::Result<u32> {
    let invalid =
      || io::Error::new(io::ErrorKind::InvalidData, "truncated RBSP");
    if count > 32 {
      return Err(invalid());
    }
    let mut next = self.clone();
    while next.available < count {
      next.cache = (next.cache << 8)
        | u64::from(next.byte()?.ok_or_else(invalid)?);
      next.available += 8;
    }
    next.available -= count;
    let value =
      ((next.cache >> next.available) & ((1u64 << count) - 1)) as u32;
    next.cache &= (1u64 << next.available) - 1;
    *self = next;
    Ok(value)
  }

  pub fn skip(&mut self, mut count: usize) -> io::Result<()> {
    let truncated =
      || io::Error::new(io::ErrorKind::InvalidData, "truncated RBSP");
    let mut next = self.clone();
    let buffered = count.min(next.available);
    next.get(buffered)?;
    count -= buffered;
    let bytes = count / 8;
    if next.zeros.is_none() {
      next.bytes = next.bytes.get(bytes..).ok_or_else(truncated)?;
    } else {
      // Advance only escape state for skipped payload. Constructing
      // and cloning a bit cache every four bytes needlessly decoded
      // large SEI payloads that the caller does not inspect.
      for _ in 0..bytes {
        next.byte()?.ok_or_else(truncated)?;
      }
    }
    next.get(count % 8)?;
    *self = next;
    Ok(())
  }

  // The signed domain needs codeNum=2^32 for i32::MIN. Decode the
  // codeword independently of either public integer representation.
  fn code(&mut self) -> io::Result<u64> {
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
    Ok((1u64 << zeros) - 1 + u64::from(self.get(zeros)?))
  }

  pub fn ue(&mut self) -> io::Result<u32> {
    u32::try_from(self.code()?)
      .map_err(|_| io::Error::other("unsigned Exp-Golomb overflow"))
  }

  pub fn ue_incremented(&mut self) -> io::Result<u32> {
    self
      .ue()?
      .checked_add(1)
      .ok_or_else(|| io::Error::other("Exp-Golomb increment overflow"))
  }

  pub fn se(&mut self) -> io::Result<i32> {
    let value = self.code()? as i64;
    let signed =
      if value & 1 != 0 { value / 2 + 1 } else { -value / 2 };
    i32::try_from(signed)
      .map_err(|_| io::Error::other("signed Exp-Golomb overflow"))
  }
}
