//! module: dma::storage; responsibility: share CPU ownership by live
//! allocation identity. operations: state; entities: Registry; tests:
//! storage_tests. Weak entries do not extend storage lifetimes, and
//! geometric collection amortizes registry scans across new imports.
use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use super::access::State;

struct Registry {
  states: BTreeMap<(u64, u64), Weak<State>>,
  collection_size: usize,
}
static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
  Mutex::new(Registry { states: BTreeMap::new(), collection_size: 64 })
});

pub(super) fn state(identity: (u64, u64)) -> io::Result<Arc<State>> {
  let mut registry = REGISTRY
    .lock()
    .map_err(|_| io::Error::other("DMA storage registry poisoned"))?;
  if let Some(state) =
    registry.states.get(&identity).and_then(Weak::upgrade)
  {
    return Ok(state);
  }
  if registry.states.len() >= registry.collection_size {
    registry.states.retain(|_, state| state.strong_count() != 0);
    registry.collection_size =
      registry.states.len().saturating_mul(2).max(64);
  }
  let state = Arc::new(State::default());
  registry.states.insert(identity, Arc::downgrade(&state));
  Ok(state)
}
