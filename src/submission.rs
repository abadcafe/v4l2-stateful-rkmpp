//! module: submission; responsibility: express compressed-input
//! ownership. operations/entities: Origin, Payload, Prefix,
//! MAX_PACKET_SIZE; tests: submission_tests, backend/input_tests and
//! frontend_tests. Payload ranges refer to the caller's retained read
//! lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Origin(u64);
impl Origin {
  pub(crate) fn new(value: u64) -> Self {
    Self(value)
  }
}
use rkmpp::{MAX_PACKET_SIZE, Payload};

/// Unsubmitted metadata. Bounds are checked before allocation or
/// mutation; the admission owner discards this prefix when its
/// transaction fails.
#[derive(Default)]
pub(crate) struct Prefix(Vec<u8>);
impl Prefix {
  pub(crate) fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
    checked_size(self.0.len(), bytes.len())?;
    self.0.extend_from_slice(bytes);
    Ok(())
  }

  pub(crate) fn clear(&mut self) {
    self.0.clear();
  }

  pub(crate) fn compose(
    &mut self,
    configuration: &[u8],
    input: &[u8],
    range: std::ops::Range<usize>,
  ) -> std::io::Result<Payload> {
    let bytes = input
      .get(range.clone())
      .filter(|bytes| !bytes.is_empty())
      .ok_or_else(|| {
        std::io::Error::new(
          std::io::ErrorKind::InvalidData,
          "invalid compressed input range",
        )
      })?;
    let prefix_size = checked_size(configuration.len(), self.0.len())?;
    checked_size(prefix_size, bytes.len())?;
    if configuration.is_empty() && self.0.is_empty() {
      return Ok(Payload::Input(range));
    }
    let prefix = if configuration.is_empty() {
      std::mem::take(&mut self.0)
    } else {
      let mut payload = Vec::with_capacity(prefix_size);
      payload.extend_from_slice(configuration);
      payload.extend_from_slice(&self.0);
      self.0.clear();
      payload
    };
    Ok(Payload::Prefixed { prefix, range })
  }
}
fn checked_size(
  prefix: usize,
  suffix: usize,
) -> std::io::Result<usize> {
  prefix
    .checked_add(suffix)
    .filter(|&size| size <= MAX_PACKET_SIZE)
    .ok_or_else(|| {
      std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "compressed task including metadata exceeds input limit",
      )
    })
}
