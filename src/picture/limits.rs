//! module: picture::limits; responsibility: validate decoded-picture
//! buffering limits. operations: new, capacity, reorder, latency;
//! entities: Limits; tests: limits_tests.
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
  capacity: usize,
  reorder: usize,
  latency: Option<u64>,
}
impl Limits {
  pub fn new(
    capacity: usize,
    reorder: usize,
    latency: Option<u64>,
  ) -> io::Result<Self> {
    if capacity > 16 || reorder > capacity {
      return Err(io::Error::other("invalid picture buffering limits"));
    }
    Ok(Self { capacity: capacity.max(1), reorder, latency })
  }

  pub fn capacity(self) -> usize {
    self.capacity
  }

  pub fn reorder(self) -> usize {
    self.reorder
  }

  pub fn latency(self) -> Option<u64> {
    self.latency
  }
}
impl Default for Limits {
  fn default() -> Self {
    Self { capacity: 16, reorder: 16, latency: None }
  }
}
