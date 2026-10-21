//! module: backend::presentation; responsibility: schedule normative
//! picture output. operations: prepare, picture, barrier, discard,
//! drain, take; entities: Presentation, Pending; tests:
//! presentation_tests.
use std::collections::VecDeque;

use crate::picture::{Boundary, Id, Plan};
struct Pending<T> {
  id: Id,
  order: i32,
  latency: u64,
  value: T,
}
pub(super) struct Presentation<T> {
  pictures: Vec<Pending<T>>,
  ready: VecDeque<T>,
  references: Vec<Id>,
  prepared: Option<Id>,
  occupied: Vec<Id>,
}
impl<T> Default for Presentation<T> {
  fn default() -> Self {
    Self {
      pictures: Vec::new(),
      ready: VecDeque::new(),
      references: Vec::new(),
      prepared: None,
      occupied: Vec::new(),
    }
  }
}
impl<T> Presentation<T> {
  fn occupancy(&mut self) -> usize {
    self.occupied.clear();
    self.occupied.extend_from_slice(&self.references);
    self
      .occupied
      .extend(self.pictures.iter().map(|picture| picture.id));
    self.occupied.sort_unstable();
    self.occupied.dedup();
    self.occupied.len()
  }

  fn bump(&mut self) -> bool {
    if self.pictures.is_empty() {
      return false;
    }
    self.ready.push_back(self.pictures.remove(0).value);
    true
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
    self.references.clear();
    self.references.extend(
      p.references().iter().copied().filter(|&id| id != p.id()),
    );
    while self.pictures.len() > p.limits().reorder()
      || p.limits().latency().is_some_and(|limit| {
        self.pictures.iter().any(|p| p.latency >= limit)
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
          .first()
          .is_none_or(|picture| picture.order > p.output().order)
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
    self.references.clear();
    self.references.extend_from_slice(p.references());
    if p.output().display {
      for pending in &mut self.pictures {
        if pending.order > p.output().order {
          pending.latency = pending.latency.saturating_add(1);
        }
      }
      let index = self
        .pictures
        .partition_point(|prior| prior.order <= p.output().order);
      self.pictures.insert(
        index,
        Pending {
          id: p.id(),
          order: p.output().order,
          latency: 0,
          value,
        },
      );
    }
    while self.pictures.len() > p.limits().reorder()
      || self.occupancy() > p.limits().capacity()
      || p.limits().latency().is_some_and(|limit| {
        self.pictures.iter().any(|p| p.latency >= limit)
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
