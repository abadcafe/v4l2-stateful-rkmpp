//! module: service::worker_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: Observed, Control, Decoder; tests:
//! test_run_requested_shutdown_does_not_exchange
//! test_run_close_drops_decoder_without_delivering_close_to_engine
//! test_run_reuses_registered_pool_and_batches_ordered_replies
//! test_run_capture_reset_replaces_capture_registration_only
//! test_run_full_reset_replaces_both_registrations
//! test_run_splits_more_than_one_batch_without_reordering
//! test_run_full_event_batch_fetches_again_without_wait
//! test_run_bounded_progress_yields_to_control_after_four_steps
//! test_run_stale_and_disconnected_registration_skip_event
//! test_run_exchange_disconnect_finishes_without_failed_reply
//! test_run_invalid_event_version_sends_failed_after_decoder_drop
//! test_run_reply_errors_reject_invalid_and_tolerate_stale_or_disconnect
//! test_run_backpressure_overflow_sends_failed_reply
//! test_run_interrupted_wait_retries_without_losing_control_fetch
//! test_run_wait_error_sends_failed_reply
//! test_run_backend_event_failure_preserves_failure_context
//! test_run_backend_progress_error_sends_failed_reply
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

use super::worker::{Channel, Engine, run};
use crate::dma::Buffer;
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::{Batch, Event, Message, Reply, VERSION};
use crate::shutdown::{Shutdown, Signals};

#[derive(Default)]
struct Observed {
  replies: Vec<Vec<Message>>,
  events: Vec<Message>,
  pools: Vec<u32>,
  waits: usize,
  steps: usize,
  dropped: bool,
}
struct Control {
  batches: VecDeque<io::Result<Batch>>,
  register_error: Option<i32>,
  waits: VecDeque<io::Result<bool>>,
  seen: Rc<RefCell<Observed>>,
}
impl Channel for Control {
  fn exchange(&mut self, batch: &mut Batch) -> io::Result<()> {
    self
      .seen
      .borrow_mut()
      .replies
      .push(batch.replies[..batch.reply_count as usize].to_vec());
    let response = self.batches.pop_front().unwrap_or_else(|| {
      Err(io::Error::from_raw_os_error(libc::EPIPE))
    })?;
    batch.event_count = response.event_count;
    batch.events = response.events;
    batch.results = response.results;
    Ok(())
  }

  fn register(
    &mut self,
    direction: Direction,
    _epoch: u64,
    id: u32,
  ) -> io::Result<Pool> {
    if let Some(error) = self.register_error {
      return Err(io::Error::from_raw_os_error(error));
    }
    Pool::new(direction, id, vec![Buffer::allocate(4096)?])
  }

  fn wait(
    &mut self,
    _notify: &Notify,
    _shutdown: &Shutdown,
  ) -> io::Result<bool> {
    self.seen.borrow_mut().waits += 1;
    self.waits.pop_front().unwrap_or(Ok(true))
  }
}
struct Decoder {
  notify: Notify,
  replies: VecDeque<Message>,
  steps: VecDeque<io::Result<bool>>,
  event_error: bool,
  seen: Rc<RefCell<Observed>>,
}
impl Engine for Decoder {
  fn notification(&self) -> Notify {
    self.notify.clone()
  }

  fn register(&mut self, pool: Pool) -> io::Result<()> {
    self.seen.borrow_mut().pools.push(pool.into_parts().1);
    Ok(())
  }

  fn event(&mut self, message: Message) -> io::Result<()> {
    self.seen.borrow_mut().events.push(message);
    if self.event_error {
      Err(io::Error::other("event rejected"))
    } else {
      Ok(())
    }
  }

  fn step(&mut self) -> io::Result<bool> {
    self.seen.borrow_mut().steps += 1;
    self.steps.pop_front().unwrap_or(Ok(false))
  }

  fn take_reply(&mut self) -> Option<Message> {
    self.replies.pop_front()
  }
}
impl Drop for Decoder {
  fn drop(&mut self) {
    self.seen.borrow_mut().dropped = true;
  }
}
fn message(kind: u32, id: u32) -> Message {
  Message {
    version: VERSION,
    kind,
    epoch: 1,
    pool_id: id,
    ..Default::default()
  }
}
fn batch(events: &[Message]) -> Batch {
  let mut batch = Batch {
    version: VERSION,
    event_count: events.len() as u32,
    ..Default::default()
  };
  batch.events[..events.len()].copy_from_slice(events);
  batch
}
fn fixture(
  batches: Vec<io::Result<Batch>>,
) -> (Control, Decoder, Rc<RefCell<Observed>>) {
  let seen = Rc::new(RefCell::new(Observed::default()));
  (
    Control {
      batches: batches.into(),
      register_error: None,
      waits: VecDeque::new(),
      seen: seen.clone(),
    },
    Decoder {
      notify: Notify::new().unwrap(),
      replies: VecDeque::new(),
      steps: VecDeque::new(),
      event_error: false,
      seen: seen.clone(),
    },
    seen,
  )
}
fn close() -> io::Result<Batch> {
  Ok(batch(&[message(Event::Close as u32, 0)]))
}

#[test]
fn test_run_requested_shutdown_does_not_exchange() {
  let (mut control, engine, seen) = fixture(vec![]);
  let signals = Signals::install().unwrap();
  signals.shutdown().request();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown()).unwrap().exchanges,
    0
  );
  assert!(seen.borrow().replies.is_empty() && seen.borrow().dropped);
}
#[test]
fn test_run_close_drops_decoder_without_delivering_close_to_engine() {
  let (mut control, engine, seen) = fixture(vec![close()]);
  let signals = Signals::install().unwrap();
  let stats = run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!((stats.exchanges, stats.events), (1, 1));
  assert!(seen.borrow().events.is_empty() && seen.borrow().dropped);
}
#[test]
fn test_run_reuses_registered_pool_and_batches_ordered_replies() {
  let packet = message(Event::Packet as u32, 7);
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[packet, packet])), close()]);
  engine.replies = vec![message(Reply::InputDone as u32, 7); 2].into();
  let signals = Signals::install().unwrap();
  let stats = run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!(
    (stats.registrations, stats.replies, stats.max_replies),
    (1, 2, 2)
  );
  let seen = seen.borrow();
  assert_eq!(seen.pools, vec![7]);
  assert_eq!(seen.events.len(), 2);
  assert_eq!(seen.replies[1].len(), 2);
}
#[test]
fn test_run_capture_reset_replaces_capture_registration_only() {
  let mut reset = message(Event::Reset as u32, 0);
  reset.flags = Message::RESET_CAPTURE_ONLY;
  let events = [
    message(Event::Packet as u32, 1),
    message(Event::Capture as u32, 2),
    reset,
    message(Event::Recycle as u32, 2),
    message(Event::Packet as u32, 1),
  ];
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&events)), close()]);
  let signals = Signals::install().unwrap();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap()
      .registrations,
    3
  );
  assert_eq!(seen.borrow().pools, vec![1, 2, 2]);
}
#[test]
fn test_run_full_reset_replaces_both_registrations() {
  let events = [
    message(Event::Packet as u32, 1),
    message(Event::Capture as u32, 2),
    message(Event::Reset as u32, 0),
    message(Event::Packet as u32, 1),
    message(Event::Recycle as u32, 2),
  ];
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&events)), close()]);
  let signals = Signals::install().unwrap();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap()
      .registrations,
    4
  );
  assert_eq!(seen.borrow().pools, vec![1, 2, 1, 2]);
}
#[test]
fn test_run_splits_more_than_one_batch_without_reordering() {
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[])), Ok(batch(&[])), close()]);
  engine.replies = (0..17)
    .map(|index| Message {
      index,
      ..message(Reply::InputDone as u32, 1)
    })
    .collect();
  let signals = Signals::install().unwrap();
  let stats = run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!((stats.replies, stats.max_replies), (17, 16));
  let seen = seen.borrow();
  assert_eq!((seen.replies[1].len(), seen.replies[2].len()), (16, 1));
  assert_eq!(
    seen.replies.iter().flatten().map(|m| m.index).collect::<Vec<_>>(),
    (0..17).collect::<Vec<_>>()
  );
}
#[test]
fn test_run_full_event_batch_fetches_again_without_wait() {
  let events = [message(Event::Resume as u32, 0); 16];
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&events)), close()]);
  let signals = Signals::install().unwrap();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown()).unwrap().max_events,
    16
  );
  assert_eq!(seen.borrow().waits, 0);
}
#[test]
fn test_run_bounded_progress_yields_to_control_after_four_steps() {
  let (mut control, mut engine, seen) = fixture(vec![
    Ok(batch(&[message(Event::Resume as u32, 0)])),
    close(),
  ]);
  engine.steps = (0..8).map(|_| Ok(true)).collect();
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!(seen.borrow().steps, 4);
}
#[test]
fn test_run_stale_and_disconnected_registration_skip_event() {
  for error in [libc::ESTALE, libc::EPIPE] {
    let (mut control, engine, seen) = fixture(vec![
      Ok(batch(&[message(Event::Packet as u32, 1)])),
      close(),
    ]);
    control.register_error = Some(error);
    let signals = Signals::install().unwrap();
    assert_eq!(
      run(&mut control, engine, &signals.shutdown())
        .unwrap()
        .registrations,
      0
    );
    assert!(seen.borrow().events.is_empty());
  }
}
#[test]
fn test_run_exchange_disconnect_finishes_without_failed_reply() {
  let (mut control, engine, seen) =
    fixture(vec![Err(io::Error::from_raw_os_error(libc::EPIPE))]);
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!(seen.borrow().replies.len(), 1);
}
#[test]
fn test_run_invalid_event_version_sends_failed_after_decoder_drop() {
  let mut event = message(Event::Packet as u32, 1);
  event.version += 1;
  let (mut control, engine, seen) = fixture(vec![Ok(batch(&[event]))]);
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap_err()
      .contains("version")
  );
  let seen = seen.borrow();
  assert!(seen.dropped);
  assert_eq!(
    seen.replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
#[test]
fn test_run_reply_errors_reject_invalid_and_tolerate_stale_or_disconnect()
 {
  for status in [-libc::EINVAL, -libc::ESTALE, -libc::EPIPE] {
    let mut response = batch(&[]);
    response.results[0] = status;
    let (mut control, mut engine, _) =
      fixture(vec![Ok(batch(&[])), Ok(response), close()]);
    engine.replies.push_back(message(Reply::InputDone as u32, 1));
    let signals = Signals::install().unwrap();
    assert_eq!(
      run(&mut control, engine, &signals.shutdown()).is_ok(),
      status != -libc::EINVAL
    );
  }
}
#[test]
fn test_run_backpressure_overflow_sends_failed_reply() {
  let (mut control, mut engine, seen) = fixture(vec![Ok(batch(&[]))]);
  engine.replies = vec![message(Reply::InputDone as u32, 1); 65].into();
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap_err()
      .contains("backpressure")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
#[test]
fn test_run_interrupted_wait_retries_without_losing_control_fetch() {
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&[])), close()]);
  control.waits =
    vec![Err(io::Error::from_raw_os_error(libc::EINTR)), Ok(true)]
      .into();
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).unwrap();
  assert_eq!(seen.borrow().waits, 2);
}
#[test]
fn test_run_wait_error_sends_failed_reply() {
  let (mut control, engine, seen) = fixture(vec![Ok(batch(&[]))]);
  control.waits.push_back(Err(io::Error::from_raw_os_error(libc::EIO)));
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap_err()
      .contains("wait")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
#[test]
fn test_run_backend_event_failure_preserves_failure_context() {
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[message(Event::Resume as u32, 0)]))]);
  engine.event_error = true;
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap_err()
      .contains("backend event")
  );
  assert_eq!(seen.borrow().replies.last().unwrap()[0].epoch, 1);
}
#[test]
fn test_run_backend_progress_error_sends_failed_reply() {
  let (mut control, mut engine, seen) = fixture(vec![Ok(batch(&[]))]);
  engine.steps.push_back(Err(io::Error::other("decode failed")));
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .unwrap_err()
      .contains("backend progress")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
