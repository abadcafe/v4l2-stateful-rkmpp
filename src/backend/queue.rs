//! module: backend::queue; responsibility: preserve input order and
//! arbitrate decode termination boundaries. operations: submit, drain,
//! expand, damaged, abort_drain, front, pop, clear, is_empty; entities:
//! Queue, Queued; tests: queue_tests and service/test_bitstream.
use std::collections::VecDeque;
use std::num::NonZeroU32;

use rkmpp::Packet;

use crate::timestamp::Source;

pub(super) enum Queued {
  Analyze {
    index: usize,
    timestamp: u64,
    source: Source,
    token: NonZeroU32,
  },
  Task {
    packet: Packet,
    serial: u64,
  },
  Release {
    index: usize,
    token: NonZeroU32,
    damaged: bool,
  },
  Eos(Option<NonZeroU32>),
}

#[derive(Default)]
pub(super) struct Queue(VecDeque<Queued>);

impl Queue {
  pub(super) fn submit(
    &mut self,
    index: usize,
    timestamp: u64,
    source: Source,
    token: NonZeroU32,
  ) {
    self.0.push_back(Queued::Analyze {
      index,
      timestamp,
      source,
      token,
    });
  }

  pub(super) fn drain(&mut self) {
    if !matches!(self.0.back(), Some(Queued::Eos(_))) {
      self.0.push_back(Queued::Eos(None));
    }
  }

  pub(super) fn expand(
    &mut self,
    tasks: &mut Vec<(Packet, u64)>,
    index: usize,
    token: NonZeroU32,
    end: bool,
  ) {
    if end {
      // STOP immediately after this input and its in-band terminator
      // delimit the same prefix. Preserve the natural boundary, whose
      // resume resets codec sequence state. Inputs between boundaries
      // make them distinct, so a later STOP must stay in order.
      if matches!(self.0.front(), Some(Queued::Eos(None))) {
        self.0.pop_front();
      }
      self.0.push_front(Queued::Eos(Some(token)));
    }
    self.0.push_front(Queued::Release { index, token, damaged: false });
    for (packet, serial) in tasks.drain(..).rev() {
      self.0.push_front(Queued::Task { packet, serial });
    }
  }

  pub(super) fn damaged(&mut self, index: usize, token: NonZeroU32) {
    self.0.push_front(Queued::Release { index, token, damaged: true });
  }

  pub(super) fn abort_drain(&mut self) {
    if let Some(index) =
      self.0.iter().position(|p| matches!(p, Queued::Eos(_)))
    {
      self.0.remove(index);
    }
  }

  pub(super) fn front(&self) -> Option<&Queued> {
    self.0.front()
  }

  pub(super) fn pop(&mut self) -> Option<Queued> {
    self.0.pop_front()
  }

  pub(super) fn clear(&mut self) {
    self.0.clear();
  }

  pub(super) fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
}
