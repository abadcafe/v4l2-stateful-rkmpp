//! module: service::scheduler_tests; responsibility: define the
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
//! test_run_rejects_reserved_before_close_registration_or_engine_mutation
//! test_run_waits_after_consumed_events_when_engine_has_no_runnable_work
//! test_run_reply_only_exchange_does_not_advance_idle_hardware
//! test_run_backend_wait_completion_advances_without_control_fetch
//! test_run_backend_wait_failure_closes_before_failed_reply
//! test_run_backend_close_failure_reports_failed_after_owner_release
//! test_run_backend_close_failure_preserves_original_wait_error
//! test_run_shutdown_wakes_wait_and_closes_before_return
//! test_run_listener_failure_closes_before_failed_reply
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

use dma_buf::memory::Buffer;
use rkmpp_bridge_uapi::wire::Flags;

use super::scheduler::{Channel, Engine, run};
use crate::notify::Notify;
use crate::pool::{Direction, Pool};
use crate::protocol::command::Command;
use crate::protocol::{Batch, Event, Message, Reply, VERSION};
use crate::shutdown::{Shutdown, Signals};

#[derive(Default)]
struct Observed {
  replies: Vec<Vec<Message>>,
  events: Vec<Command>,
  pools: Vec<u32>,
  waits: usize,
  steps: usize,
  advances: usize,
  closed: bool,
  replies_after_close: Vec<bool>,
  dropped: bool,
}
struct Control {
  batches: VecDeque<io::Result<Batch>>,
  register_error: Option<i32>,
  waits: VecDeque<io::Result<bool>>,
  block_wait: bool,
  seen: Rc<RefCell<Observed>>,
}
impl Channel for Control {
  async fn exchange(&mut self, batch: &mut Batch) -> io::Result<()> {
    let closed = {
      let seen = self.seen.borrow();
      seen.closed && seen.dropped
    };
    self.seen.borrow_mut().replies_after_close.push(closed);
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

  async fn register(
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

  async fn wait(
    &mut self,
    _notification: &crate::notify::Listener,
    shutdown: &Shutdown,
  ) -> io::Result<bool> {
    self.seen.borrow_mut().waits += 1;
    if self.block_wait {
      shutdown.cancelled().await;
      return Ok(false);
    }
    self.waits.pop_front().unwrap_or(Ok(true))
  }
}
struct Decoder {
  notify: Notify,
  replies: VecDeque<Message>,
  steps: VecDeque<io::Result<bool>>,
  event_error: bool,
  waits: RefCell<VecDeque<io::Result<()>>>,
  close_error: Option<&'static str>,
  reply_on_advance: Option<(usize, Message)>,
  seen: Rc<RefCell<Observed>>,
}
impl Engine for Decoder {
  async fn close(self) -> io::Result<()> {
    let error = self.close_error;
    self.seen.borrow_mut().closed = true;
    // Teardown suspends before releasing its owner. A FAILED reply
    // must await the complete close transaction.
    tokio::task::yield_now().await;
    drop(self);
    match error {
      Some(message) => Err(io::Error::other(message)),
      None => Ok(()),
    }
  }

  fn notification(&self) -> Notify {
    self.notify.clone()
  }

  fn submit(
    &mut self,
    command: Command,
    pool: Option<Pool>,
  ) -> io::Result<()> {
    if let Some(pool) = pool {
      self.seen.borrow_mut().pools.push(pool.into_parts().1);
    }
    self.seen.borrow_mut().events.push(command);
    if self.event_error {
      Err(io::Error::other("event rejected"))
    } else {
      Ok(())
    }
  }

  async fn advance(&mut self) -> io::Result<bool> {
    self.seen.borrow_mut().advances += 1;
    if let Some((turn, reply)) = self.reply_on_advance
      && turn == self.seen.borrow().advances
    {
      self.replies.push_back(reply);
      self.reply_on_advance = None;
    }
    let mut progressed = false;
    for _ in 0..4 {
      self.seen.borrow_mut().steps += 1;
      progressed = self.steps.pop_front().unwrap_or(Ok(false))?;
      if !progressed {
        break;
      }
    }
    Ok(progressed)
  }

  async fn wait(&self) -> io::Result<()> {
    let result = self.waits.borrow_mut().pop_front();
    match result {
      Some(result) => result,
      None => std::future::pending().await,
    }
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
    count: 1,
    bytesused: 1,
    codec: u32::from_le_bytes(*b"H264"),
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
      block_wait: false,
      seen: seen.clone(),
    },
    Decoder {
      notify: Notify::new().unwrap(),
      replies: VecDeque::new(),
      steps: VecDeque::new(),
      event_error: false,
      waits: RefCell::default(),
      close_error: None,
      reply_on_advance: None,
      seen: seen.clone(),
    },
    seen,
  )
}
fn close() -> io::Result<Batch> {
  Ok(batch(&[message(Event::Close as u32, 0)]))
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_requested_shutdown_does_not_exchange() {
  let (mut control, engine, seen) = fixture(vec![]);
  let signals = Signals::install().unwrap();
  signals.shutdown().request();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap()
      .exchanges,
    0
  );
  assert!(seen.borrow().replies.is_empty() && seen.borrow().dropped);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_close_drops_decoder_without_delivering_close_to_engine()
 {
  let (mut control, engine, seen) = fixture(vec![close()]);
  let signals = Signals::install().unwrap();
  let stats =
    run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!((stats.exchanges, stats.events), (1, 1));
  assert!(seen.borrow().events.is_empty() && seen.borrow().dropped);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_reuses_registered_pool_and_batches_ordered_replies() {
  let packet = message(Event::Packet as u32, 7);
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[packet, packet])), close()]);
  engine.replies = vec![message(Reply::InputDone as u32, 7); 2].into();
  let signals = Signals::install().unwrap();
  let stats =
    run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!(
    (stats.registrations, stats.replies, stats.max_replies),
    (1, 2, 2)
  );
  let seen = seen.borrow();
  assert_eq!(seen.pools, vec![7]);
  assert_eq!(seen.events.len(), 2);
  assert_eq!(seen.advances, 1);
  assert_eq!(seen.replies[1].len(), 2);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_capture_reset_replaces_capture_registration_only() {
  let mut reset = message(Event::Reset as u32, 0);
  reset.flags = Flags::ResetCaptureOnly as u32;
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
      .await
      .unwrap()
      .registrations,
    3
  );
  assert_eq!(seen.borrow().pools, vec![1, 2, 2]);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_full_reset_replaces_both_registrations() {
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
      .await
      .unwrap()
      .registrations,
    4
  );
  assert_eq!(seen.borrow().pools, vec![1, 2, 1, 2]);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_splits_more_than_one_batch_without_reordering() {
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[])), Ok(batch(&[])), close()]);
  engine.replies = (0..17)
    .map(|index| Message {
      index,
      ..message(Reply::InputDone as u32, 1)
    })
    .collect();
  let signals = Signals::install().unwrap();
  let stats =
    run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!((stats.replies, stats.max_replies), (17, 16));
  let seen = seen.borrow();
  assert_eq!((seen.replies[1].len(), seen.replies[2].len()), (16, 1));
  assert_eq!(
    seen.replies.iter().flatten().map(|m| m.index).collect::<Vec<_>>(),
    (0..17).collect::<Vec<_>>()
  );
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_full_event_batch_fetches_again_without_wait() {
  let events = [message(Event::Resume as u32, 0); 16];
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&events)), close()]);
  let signals = Signals::install().unwrap();
  assert_eq!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap()
      .max_events,
    16
  );
  assert_eq!(seen.borrow().waits, 0);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_bounded_progress_yields_to_control_after_four_steps()
{
  let (mut control, mut engine, seen) = fixture(vec![
    Ok(batch(&[message(Event::Resume as u32, 0)])),
    close(),
  ]);
  engine.steps = (0..8).map(|_| Ok(true)).collect();
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!(seen.borrow().steps, 4);
  assert_eq!(seen.borrow().advances, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_runnable_hardware_does_not_exchange_without_kernel_readiness()
 {
  let (mut control, mut engine, seen) = fixture(vec![
    Ok(batch(&[message(Event::Resume as u32, 0)])),
    close(),
  ]);
  // No kernel event becomes readable while the decoder needs three
  // bounded turns. The final reply itself authorizes the next ioctl.
  control.block_wait = true;
  engine.steps = (0..8).map(|_| Ok(true)).chain([Ok(false)]).collect();
  engine.reply_on_advance =
    Some((3, message(Reply::InputDone as u32, 1)));
  let signals = Signals::install().unwrap();
  let stats = tokio::time::timeout(
    std::time::Duration::from_secs(2),
    run(&mut control, engine, &signals.shutdown()),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!((stats.exchanges, stats.replies), (2, 1));
  let observed = seen.borrow();
  assert_eq!(observed.advances, 3);
  assert_eq!(observed.replies[1][0].kind, Reply::InputDone as u32);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_stale_and_disconnected_registration_skip_event() {
  for error in [libc::ESTALE, libc::EPIPE] {
    let (mut control, engine, seen) = fixture(vec![
      Ok(batch(&[message(Event::Packet as u32, 1)])),
      close(),
    ]);
    control.register_error = Some(error);
    let signals = Signals::install().unwrap();
    assert_eq!(
      run(&mut control, engine, &signals.shutdown())
        .await
        .unwrap()
        .registrations,
      0
    );
    assert!(seen.borrow().events.is_empty());
  }
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_exchange_disconnect_finishes_without_failed_reply() {
  let (mut control, engine, seen) =
    fixture(vec![Err(io::Error::from_raw_os_error(libc::EPIPE))]);
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!(seen.borrow().replies.len(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_invalid_event_version_sends_failed_after_decoder_drop()
 {
  let mut event = message(Event::Packet as u32, 1);
  event.version += 1;
  let (mut control, engine, seen) = fixture(vec![Ok(batch(&[event]))]);
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .await
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
#[tokio::test(flavor = "current_thread")]
async fn test_run_reply_errors_reject_invalid_and_tolerate_stale_or_disconnect()
 {
  for status in [-libc::EINVAL, -libc::ESTALE, -libc::EPIPE] {
    let mut response = batch(&[]);
    response.results[0] = status;
    let (mut control, mut engine, _) =
      fixture(vec![Ok(batch(&[])), Ok(response), close()]);
    engine.replies.push_back(message(Reply::InputDone as u32, 1));
    let signals = Signals::install().unwrap();
    assert_eq!(
      run(&mut control, engine, &signals.shutdown()).await.is_ok(),
      status != -libc::EINVAL
    );
  }
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_backpressure_overflow_sends_failed_reply() {
  let (mut control, mut engine, seen) = fixture(vec![Ok(batch(&[]))]);
  engine.replies = vec![message(Reply::InputDone as u32, 1); 65].into();
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap_err()
      .contains("backpressure")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_interrupted_wait_retries_without_losing_control_fetch()
 {
  let (mut control, engine, seen) =
    fixture(vec![Ok(batch(&[])), close()]);
  control.waits =
    vec![Err(io::Error::from_raw_os_error(libc::EINTR)), Ok(true)]
      .into();
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!(seen.borrow().waits, 2);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_wait_error_sends_failed_reply() {
  let (mut control, engine, seen) = fixture(vec![Ok(batch(&[]))]);
  control.waits.push_back(Err(io::Error::from_raw_os_error(libc::EIO)));
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap_err()
      .contains("wait")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_event_failure_preserves_failure_context() {
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[message(Event::Resume as u32, 0)]))]);
  engine.event_error = true;
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap_err()
      .contains("backend event")
  );
  assert_eq!(seen.borrow().replies.last().unwrap()[0].epoch, 1);
}
#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_progress_error_sends_failed_reply() {
  let (mut control, mut engine, seen) = fixture(vec![Ok(batch(&[]))]);
  engine.steps.push_back(Err(io::Error::other("decode failed")));
  let signals = Signals::install().unwrap();
  assert!(
    run(&mut control, engine, &signals.shutdown())
      .await
      .unwrap_err()
      .contains("backend progress")
  );
  assert_eq!(
    seen.borrow().replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_rejects_reserved_before_close_registration_or_engine_mutation()
 {
  for kind in [
    Event::Packet,
    Event::Capture,
    Event::Recycle,
    Event::Drain,
    Event::Reset,
    Event::Close,
    Event::Resume,
  ] {
    let mut invalid = message(kind as u32, 9);
    invalid.reserved = [1];
    let (mut control, engine, seen) =
      fixture(vec![Ok(batch(&[invalid]))]);
    let signals = Signals::install().unwrap();
    assert!(
      run(&mut control, engine, &signals.shutdown()).await.is_err()
    );
    let seen = seen.borrow();
    assert!(seen.events.is_empty() && seen.pools.is_empty());
    assert!(seen.dropped);
    let failed = seen.replies.last().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].kind, Reply::Failed as u32);
    assert_eq!(failed[0].reserved, [0]);
    assert_eq!(failed[0].epoch, invalid.epoch);
  }
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_waits_after_consumed_events_when_engine_has_no_runnable_work()
 {
  let (mut control, engine, seen) = fixture(vec![
    Ok(batch(&[message(Event::Resume as u32, 0)])),
    close(),
  ]);
  let signals = Signals::install().unwrap();
  run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!(seen.borrow().advances, 1);
  assert_eq!(seen.borrow().waits, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_reply_only_exchange_does_not_advance_idle_hardware() {
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[])), Ok(batch(&[])), close()]);
  engine.replies.push_back(message(Reply::InputDone as u32, 1));
  let signals = Signals::install().unwrap();
  let stats =
    run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!((stats.exchanges, stats.replies), (3, 1));
  assert_eq!(seen.borrow().advances, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_wait_completion_advances_without_control_fetch()
 {
  let (mut control, mut engine, seen) = fixture(vec![
    Ok(batch(&[message(Event::Resume as u32, 0)])),
    close(),
  ]);
  control.block_wait = true;
  engine.waits.get_mut().push_back(Ok(()));
  engine.reply_on_advance =
    Some((2, message(Reply::InputDone as u32, 1)));
  let signals = Signals::install().unwrap();
  let stats =
    run(&mut control, engine, &signals.shutdown()).await.unwrap();
  assert_eq!((stats.exchanges, stats.replies), (2, 1));
  let seen = seen.borrow();
  assert_eq!(seen.advances, 2);
  assert_eq!(seen.replies[1][0].kind, Reply::InputDone as u32);
  assert!(seen.closed && seen.dropped);
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_wait_failure_closes_before_failed_reply() {
  let mut event = message(Event::Resume as u32, 0);
  event.generation = 9;
  let (mut control, mut engine, seen) =
    fixture(vec![Ok(batch(&[event]))]);
  control.block_wait = true;
  engine
    .waits
    .get_mut()
    .push_back(Err(io::Error::other("completion lost")));
  let signals = Signals::install().unwrap();
  let error =
    run(&mut control, engine, &signals.shutdown()).await.unwrap_err();
  assert!(error.contains("wait: completion lost"));
  let seen = seen.borrow();
  assert!(seen.closed && seen.dropped);
  assert_eq!(seen.replies_after_close, [false, true]);
  let failed = seen.replies.last().unwrap();
  assert_eq!(failed.len(), 1);
  assert_eq!(
    (failed[0].kind, failed[0].epoch, failed[0].generation),
    (Reply::Failed as u32, event.epoch, event.generation)
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_close_failure_reports_failed_after_owner_release()
 {
  let (mut control, mut engine, seen) = fixture(vec![close()]);
  engine.close_error = Some("teardown failed");
  let signals = Signals::install().unwrap();
  let error =
    run(&mut control, engine, &signals.shutdown()).await.unwrap_err();
  assert!(error.contains("close backend: teardown failed"));
  let seen = seen.borrow();
  assert!(seen.closed && seen.dropped);
  assert_eq!(seen.replies_after_close, [false, true]);
  assert_eq!(
    seen.replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_backend_close_failure_preserves_original_wait_error()
{
  let (mut control, mut engine, seen) = fixture(vec![Ok(batch(&[]))]);
  control.block_wait = true;
  engine
    .waits
    .get_mut()
    .push_back(Err(io::Error::other("completion lost")));
  engine.close_error = Some("teardown failed");
  let signals = Signals::install().unwrap();
  let error =
    run(&mut control, engine, &signals.shutdown()).await.unwrap_err();
  assert_eq!(error, "wait: completion lost");
  let seen = seen.borrow();
  assert!(seen.closed && seen.dropped);
  assert_eq!(seen.replies_after_close, [false, true]);
  assert_eq!(
    seen.replies.last().unwrap()[0].kind,
    Reply::Failed as u32
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_shutdown_wakes_wait_and_closes_before_return() {
  let (mut control, engine, seen) = fixture(vec![Ok(batch(&[]))]);
  control.block_wait = true;
  let signals = Signals::install().unwrap();
  let shutdown = signals.shutdown();
  let stop = async {
    while seen.borrow().waits == 0 {
      tokio::task::yield_now().await;
    }
    shutdown.request();
  };
  let (result, ()) =
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      tokio::join!(run(&mut control, engine, &shutdown), stop)
    })
    .await
    .unwrap();
  let stats = result.unwrap();
  assert_eq!((stats.exchanges, stats.events, stats.replies), (1, 0, 0));
  let seen = seen.borrow();
  assert!(seen.closed && seen.dropped);
  assert_eq!(seen.advances, 1);
  assert_eq!(seen.replies_after_close, [false]);
}

#[tokio::test(flavor = "current_thread")]
async fn test_run_listener_failure_closes_before_failed_reply() {
  let (mut control, mut engine, seen) = fixture(vec![]);
  // Occupy the registration through the notification's public API.
  // The scheduler must handle this setup error like later errors.
  let listener = engine.notification().listen().unwrap();
  engine.close_error = Some("teardown failed");
  let signals = Signals::install().unwrap();
  let error =
    run(&mut control, engine, &signals.shutdown()).await.unwrap_err();
  assert_eq!(
    error,
    io::Error::from_raw_os_error(libc::EEXIST).to_string()
  );
  let seen = seen.borrow();
  assert!(seen.closed && seen.dropped);
  assert!(seen.events.is_empty() && seen.pools.is_empty());
  assert_eq!((seen.advances, seen.waits), (0, 0));
  assert_eq!(seen.replies_after_close, [true]);
  assert_eq!(seen.replies.len(), 1);
  assert_eq!(seen.replies[0].len(), 1);
  assert_eq!(seen.replies[0][0].kind, Reply::Failed as u32);
  drop(listener);
}
