//! module: dma_buf::device; responsibility: reserve local storage
//! access for device work. operations: Storage::new/metadata/reserve,
//! Access drop; entities: Access, Storage, DeviceOwner; tests:
//! device_tests. This is an access reservation,
//! not an allocation owner or a device-completion fence. The work owner
//! retains its allocation and this reservation until device completion.
use std::io;
use std::sync::Arc;

use super::access::{Lease, Owner, State};
use super::storage;
use super::system::Metadata;

pub struct Storage {
  metadata: Metadata,
  state: Arc<State>,
}
struct DeviceOwner(Arc<State>);
impl Owner for DeviceOwner {
  fn state(&self) -> &State {
    &self.0
  }

  fn synchronize(&self, _start: bool, _write: bool) -> io::Result<()> {
    // Device access uses explicit device fences and ownership barriers.
    // CPU cache START/END operations belong only to CPU access owners.
    Ok(())
  }
}
pub struct Access {
  _lease: Lease<DeviceOwner>,
}
impl Storage {
  pub fn new(metadata: Metadata) -> io::Result<Self> {
    Ok(Self { metadata, state: storage::state(metadata.identity())? })
  }

  pub fn metadata(&self) -> Metadata {
    self.metadata
  }

  pub fn reserve(&self) -> io::Result<Access> {
    Ok(Access {
      _lease: Lease::new(DeviceOwner(self.state.clone()), false)?,
    })
  }
}
