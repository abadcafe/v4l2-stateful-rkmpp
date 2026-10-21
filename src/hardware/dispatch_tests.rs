//! module: hardware::dispatch_tests; responsibility: define bounded
//! ordered execution.
use super::dispatch::Dispatch;
use crate::backend::Backend;
use crate::protocol::command::Command;
use crate::protocol::{Event, Message, Reply, VERSION};
fn reset(epoch: u64) -> Command {
  Command::try_from(Message {
    version: VERSION,
    kind: Event::Reset as u32,
    count: 1,
    epoch,
    ..Default::default()
  })
  .unwrap()
}
#[test]
fn test_advance_executes_ordered_batch_and_reuses_storage() {
  let mut dispatch = Dispatch::default();
  let mut backend =
    Backend::new(crate::quantize::Factory::default()).unwrap();
  for round in 1..=3 {
    for epoch in 1..=16 {
      dispatch.submit(reset(epoch + round * 16), None).unwrap();
    }
    assert!(!dispatch.advance(&mut backend).unwrap());
    let reply = dispatch.take_reply().unwrap();
    assert_eq!(
      (reply.kind, reply.epoch),
      (Reply::Stopped as u32, 16 + round * 16)
    );
    assert!(dispatch.take_reply().is_none());
  }
}
#[test]
fn test_event_enforces_bounded_pending_work() {
  let mut dispatch = Dispatch::default();
  for epoch in 0..16 {
    dispatch.submit(reset(epoch), None).unwrap();
  }
  assert!(dispatch.submit(reset(16), None).is_err());
}
