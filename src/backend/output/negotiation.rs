//! module: output::negotiation; responsibility: admit one CAPTURE
//! layout transaction. operations: announce/start/stop/commit and state
//! queries; entities: Negotiation, Phase, Streaming; tests:
//! negotiation_tests.
use std::io;
#[derive(Default)]
enum Phase {
  #[default]
  Unconfigured,
  Waiting {
    retiring: bool,
    acknowledge: bool,
  },
  Active,
}
#[derive(Default, PartialEq)]
enum Streaming {
  #[default]
  Started,
  Stopped,
}
#[derive(Default)]
pub(super) struct Negotiation {
  phase: Phase,
  streaming: Streaming,
  expected: usize,
}
impl Negotiation {
  pub(super) fn announce(&mut self, retiring: bool, acknowledge: bool) {
    self.phase = Phase::Waiting { retiring, acknowledge };
  }

  pub(super) fn retire(&mut self) {
    self.announce(true, self.acknowledge());
  }

  pub(super) fn start(
    &mut self,
    count: usize,
    reuse: bool,
  ) -> io::Result<()> {
    if !(1..=32).contains(&count) || (reuse && !self.waiting()) {
      return Err(io::Error::other("invalid CAPTURE activation"));
    }
    if reuse {
      self.announce(false, self.acknowledge());
    }
    self.expected = count;
    self.streaming = Streaming::Started;
    Ok(())
  }

  pub(super) fn stop(&mut self) {
    self.expected = 0;
    self.streaming = Streaming::Stopped;
    self.announce(false, self.acknowledge());
  }

  pub(super) fn commit(&mut self, registered: usize) -> bool {
    if self.active()
      || self.paused()
      || self.retiring()
      || self.expected == 0
      || registered < self.expected
    {
      return false;
    }
    self.phase = Phase::Active;
    true
  }

  pub(super) fn acknowledge(&self) -> bool {
    matches!(self.phase, Phase::Waiting { acknowledge: true, .. })
  }

  pub(super) fn retiring(&self) -> bool {
    matches!(self.phase, Phase::Waiting { retiring: true, .. })
  }

  pub(super) fn waiting(&self) -> bool {
    matches!(self.phase, Phase::Waiting { .. })
  }

  pub(super) fn active(&self) -> bool {
    matches!(self.phase, Phase::Active)
  }

  pub(super) fn paused(&self) -> bool {
    self.streaming == Streaming::Stopped
  }
}
