//! module: h264::number; responsibility: represent a valid cyclic H.264
//! frame number. operations: new, value, modulus, advance; entities:
//! Number; tests: number_tests. Construction establishes the bit-width
//! and value bounds before any shift, remainder or state transition.
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Number {
  value: u32,
  bits: u8,
}
impl Default for Number {
  fn default() -> Self {
    Self { value: 0, bits: 4 }
  }
}
impl Number {
  pub(super) fn new(value: u32, bits: usize) -> io::Result<Self> {
    if !(4..=16).contains(&bits) || value >= 1u32 << bits {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid H264 frame number",
      ));
    }
    Ok(Self { value, bits: bits as u8 })
  }

  pub(super) fn value(self) -> u32 {
    self.value
  }

  pub(super) fn modulus(self) -> u32 {
    1 << self.bits
  }

  pub(super) fn advance(self, count: u32) -> Self {
    Self {
      value: (self.value + count % self.modulus()) % self.modulus(),
      ..self
    }
  }
}
