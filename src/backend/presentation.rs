//! module: backend::presentation; responsibility: schedule normative
//! picture output. operations: prepare, picture, barrier, discard,
//! drain, take; entities: Presentation, Pending; tests:
//! presentation_tests.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::picture::{Boundary, Id, Plan};
struct Pending<T> {
  id: Id,
  latency: u64,
  value: T,
}
pub(super) struct Presentation<T> {
  pictures: BTreeMap<(i32, u64), Pending<T>>,
  ready: VecDeque<T>,
  references: BTreeSet<Id>,
  prepared: Option<Id>,
  serial: u64,
}
impl<T> Default for Presentation<T> {
  fn default() -> Self {
    Self {
      pictures: BTreeMap::new(),
      ready: VecDeque::new(),
      references: BTreeSet::new(),
      prepared: None,
      serial: 0,
    }
  }
}
impl<T> Presentation<T> {
  fn occupancy(&self) -> usize {
    self
      .references
      .iter()
      .copied()
      .chain(self.pictures.values().map(|p| p.id))
      .collect::<BTreeSet<_>>()
      .len()
  }

  fn bump(&mut self) -> bool {
    if let Some((_, p)) = self.pictures.pop_first() {
      self.ready.push_back(p.value);
      true
    } else {
      false
    }
  }

  pub(super) fn prepare(&mut self, p: &Plan) -> bool {
    if self.prepared == Some(p.id()) {
      return false;
    }
    self.prepared = Some(p.id());
    let mut progress = false;
    if p.output().before == Boundary::Discard {
      progress = !self.pictures.is_empty();
      self.discard();
    } else if p.output().before == Boundary::Drain {
      progress = !self.pictures.is_empty();
      self.drain();
    }
    self.references = p
      .references()
      .iter()
      .copied()
      .filter(|&id| id != p.id())
      .collect();
    while self.pictures.len() > p.limits().reorder()
      || p.limits().latency().is_some_and(|limit| {
        self.pictures.values().any(|p| p.latency >= limit)
      })
    {
      if !self.bump() {
        break;
      }
      progress = true;
    }
    while self.occupancy() >= p.limits().capacity() {
      if !p.reference()
        && self
          .pictures
          .first_key_value()
          .is_none_or(|((order, _), _)| *order > p.output().order)
      {
        break;
      }
      if !self.bump() {
        break;
      }
      progress = true;
    }
    progress
  }

  pub(super) fn picture(&mut self, value: T, p: &Plan) {
    self.references = p.references().iter().copied().collect();
    if p.output().display {
      for ((order, _), pending) in &mut self.pictures {
        if *order > p.output().order {
          pending.latency = pending.latency.saturating_add(1);
        }
      }
      self.serial += 1;
      self.pictures.insert(
        (p.output().order, self.serial),
        Pending { id: p.id(), latency: 0, value },
      );
    }
    while self.pictures.len() > p.limits().reorder()
      || self.occupancy() > p.limits().capacity()
      || p.limits().latency().is_some_and(|limit| {
        self.pictures.values().any(|p| p.latency >= limit)
      })
    {
      if !self.bump() {
        break;
      }
    }
  }

  pub(super) fn discard(&mut self) {
    self.pictures.clear();
  }

  pub(super) fn drain(&mut self) {
    while self.bump() {}
  }

  pub(super) fn barrier(&mut self, value: T) {
    self.drain();
    self.ready.push_back(value);
  }

  pub(super) fn take(&mut self) -> Option<T> {
    self.ready.pop_front()
  }
}
