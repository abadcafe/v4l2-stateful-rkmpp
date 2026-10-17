//! module: submission; responsibility: express compressed-input
//! ownership. operations/entities: Origin, Payload, MAX_PACKET_SIZE;
//! tests: backend/input_tests and frontend_tests. Payload ranges refer
//! to the caller's retained read lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Origin(u64);
impl Origin {
  pub(crate) fn new(value: u64) -> Self {
    Self(value)
  }
}
#[derive(Debug)]
pub enum Payload {
  Input(std::ops::Range<usize>),
  Owned(Vec<u8>),
}

/// Maximum compressed task size, including retained metadata.
pub(crate) const MAX_PACKET_SIZE: usize = 16 * 1024 * 1024;
