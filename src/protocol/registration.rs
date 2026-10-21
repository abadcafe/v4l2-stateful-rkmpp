//! module: protocol::registration; responsibility: initialize service
//! registration requests. operations: request, Pool, Descriptor,
//! MAX_BUFFERS; entities: raw wire records; tests: registration_tests
//! and transport registration acceptance.
pub use super::wire::{Descriptor, Pool};
pub const MAX_BUFFERS: usize = super::wire::RB_MAX_BUFFERS as usize;
pub fn request(kind: u32, epoch: u64, pool_id: u32) -> Pool {
  Pool {
    version: super::VERSION,
    kind,
    epoch,
    pool_id,
    buffers: [Descriptor { fd: -1, size: 0 }; MAX_BUFFERS],
    ..Default::default()
  }
}
