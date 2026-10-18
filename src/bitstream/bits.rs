//! module: bitstream::bits; responsibility: read bounded RBSP syntax.
//! operations: new, get, skip, ue, se; entities: Bits; tests:
//! bits_tests.
use std::io;
#[derive(Clone)]
pub(super) struct Bits<'a> {
  bytes: &'a [u8],
  zeros: Option<u8>,
  cache: u64,
  available: usize,
}
impl<'a> Bits<'a> {
  pub(super) fn new(data: &'a [u8]) -> Self {
    Self { bytes: data, zeros: Some(0), cache: 0, available: 0 }
  }

  pub(super) fn from_rbsp(data: &'a [u8]) -> Self {
    Self { bytes: data, zeros: None, cache: 0, available: 0 }
  }

  fn byte(&mut self) -> Option<u8> {
    loop {
      let (&value, rest) = self.bytes.split_first()?;
      self.bytes = rest;
      if let Some(zeros) = &mut self.zeros {
        if *zeros >= 2 && value == 3 {
          *zeros = 0;
          continue;
        }
        *zeros = if value == 0 { zeros.saturating_add(1) } else { 0 };
      }
      return Some(value);
    }
  }

  pub(super) fn more_rbsp_data(&self) -> bool {
    let mut remaining = self.clone();
    match remaining.get(1) {
      Err(_) => false,
      Ok(0) => true,
      Ok(_) => {
        while let Ok(bit) = remaining.get(1) {
          if bit != 0 {
            return true;
          }
        }
        false
      }
    }
  }

  pub(super) fn finish_rbsp(&mut self) -> io::Result<()> {
    if self.get(1)? != 1 {
      return Err(io::Error::other("missing RBSP stop bit"));
    }
    while let Ok(bit) = self.get(1) {
      if bit != 0 {
        return Err(io::Error::other("invalid RBSP padding"));
      }
    }
    Ok(())
  }

  pub(super) fn get(&mut self, count: usize) -> io::Result<u32> {
    let invalid =
      || io::Error::new(io::ErrorKind::InvalidData, "truncated RBSP");
    if count > 32 {
      return Err(invalid());
    }
    let mut next = self.clone();
    while next.available < count {
      next.cache =
        (next.cache << 8) | u64::from(next.byte().ok_or_else(invalid)?);
      next.available += 8;
    }
    next.available -= count;
    let value =
      ((next.cache >> next.available) & ((1u64 << count) - 1)) as u32;
    next.cache &= (1u64 << next.available) - 1;
    *self = next;
    Ok(value)
  }

  pub(super) fn skip(&mut self, mut count: usize) -> io::Result<()> {
    while count != 0 {
      let chunk = count.min(32);
      self.get(chunk)?;
      count -= chunk;
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
