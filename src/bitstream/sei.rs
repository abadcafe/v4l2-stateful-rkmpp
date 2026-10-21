//! module: bitstream::sei; responsibility: frame bounded SEI messages
//! independently of payload semantics without payload allocations.
//! Returned readers borrow input and cannot cross message boundaries.
//! operations: new, next;
//! entities: Messages, Message; tests: sei_tests.
use std::io;

use codec_parser::syntax::bits::Bits;

pub(super) struct Message<'a> {
  pub kind: u32,
  pub payload: Bits<'a>,
}
pub(super) struct Messages<'a> {
  bits: Bits<'a>,
  finished: bool,
}
fn extended(bits: &mut Bits) -> io::Result<u32> {
  let mut value = 0u32;
  loop {
    let byte = bits.get(8)?;
    value = value
      .checked_add(byte)
      .ok_or_else(|| io::Error::other("SEI header overflow"))?;
    if byte != 255 {
      return Ok(value);
    }
  }
}
impl<'a> Messages<'a> {
  pub(super) fn new(data: &'a [u8]) -> Self {
    Self { bits: Bits::new(data), finished: false }
  }

  pub(super) fn next(&mut self) -> io::Result<Option<Message<'a>>> {
    if self.finished {
      return Ok(None);
    }
    if !self.bits.more_rbsp_data() {
      self.bits.finish_rbsp()?;
      self.finished = true;
      return Ok(None);
    }
    let kind = extended(&mut self.bits)?;
    let length = extended(&mut self.bits)?;
    let payload = self.bits.take_bytes(length as usize)?;
    Ok(Some(Message { kind, payload }))
  }
}
