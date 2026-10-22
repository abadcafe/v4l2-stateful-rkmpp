//! module: dma_buf::memory::persistent; responsibility: retain checked
//! CPU reads over reusable DMA mappings. operations: new, read, bytes,
//! finish; entities: InputMap, ReadAccess; tests: persistent_tests.
use std::io;
use std::rc::Rc;

use super::{Buffer, CpuAccess, Map};

pub struct InputMap(Map);
impl InputMap {
  pub fn new(buffer: Buffer) -> io::Result<Self> {
    Map::new(buffer, false).map(Self)
  }

  pub fn read(
    self: &Rc<Self>,
    offset: usize,
    length: usize,
  ) -> io::Result<ReadAccess> {
    let end = offset
      .checked_add(length)
      .filter(|&end| length != 0 && end <= self.0.buffer.size())
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidInput,
          "packet length outside registered buffer",
        )
      })?;
    Ok(ReadAccess {
      map: self.clone(),
      lease: self.0.buffer.begin(false)?,
      range: offset..end,
    })
  }
}

// Only a read access exposes bytes: the mapping, bounds and CPU lease
// remain inseparable for the lifetime of every returned slice.
pub struct ReadAccess {
  map: Rc<InputMap>,
  lease: CpuAccess,
  range: std::ops::Range<usize>,
}
impl ReadAccess {
  pub fn bytes(&self) -> &[u8] {
    &self.map.0.bytes()[self.range.clone()]
  }

  pub fn finish(self) -> io::Result<()> {
    self.lease.finish()
  }
}
