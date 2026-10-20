//! module: hardware_tests; responsibility: define hardware executor
//! ownership. operations: tests; entities: none; tests: independent
//! sessions, cancellation, executor shutdown, closed client.
use crate::hardware::Hardware;
#[tokio::test(flavor = "current_thread")]
async fn test_open_independent_sessions_close_before_executor() {
  let hardware = Hardware::start();
  let client = hardware.client();
  let mut first = client.open(()).await.unwrap();
  let second = client.open(()).await.unwrap();
  assert!(!first.advance().await.unwrap());
  assert!(first.take_reply().is_none());
  first.close().await.unwrap();
  second.close().await.unwrap();
  hardware.close().await.unwrap();
  assert!(client.open(()).await.is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn test_close_reclaims_abandoned_sessions() {
  let hardware = Hardware::start();
  let engine = hardware.client().open(()).await.unwrap();
  drop(engine);
  hardware.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn test_close_invalidates_outstanding_engine() {
  let hardware = Hardware::start();
  let mut engine = hardware.client().open(()).await.unwrap();
  hardware.close().await.unwrap();
  assert!(engine.advance().await.is_err());
  assert!(engine.close().await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn test_drop_executor_closes_clients_with_live_engines() {
  let hardware = Hardware::start();
  let client = hardware.client();
  let mut engine = client.open(()).await.unwrap();
  drop(hardware);
  tokio::time::timeout(std::time::Duration::from_secs(2), async {
    while engine.advance().await.is_ok() {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert!(client.open(()).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn test_cancel_queued_opens_preserves_executor_admission() {
  let hardware = Hardware::start();
  let client = hardware.client();
  let mut tasks = tokio::task::JoinSet::new();
  for _ in 0..128 {
    let client = client.clone();
    tasks.spawn(async move { client.open(()).await });
  }
  tokio::task::yield_now().await;
  tasks.abort_all();
  while tasks.join_next().await.is_some() {}
  let mut engine = client.open(()).await.unwrap();
  assert!(!engine.advance().await.unwrap());
  engine.close().await.unwrap();
  hardware.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_close_releases_channel_lease_only_after_hardware_teardown()
 {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};
  struct Lease(Arc<AtomicBool>);
  impl Drop for Lease {
    fn drop(&mut self) {
      self.0.store(true, Ordering::Release);
    }
  }
  let released = Arc::new(AtomicBool::new(false));
  let hardware = Hardware::start();
  let engine =
    hardware.client().open(Lease(released.clone())).await.unwrap();
  assert!(!released.load(Ordering::Acquire));
  engine.close().await.unwrap();
  assert!(released.load(Ordering::Acquire));
  hardware.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_cancel_completed_open_reclaims_lease_while_executor_is_idle()
 {
  use std::future::Future;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::task::{Context, Waker};
  struct Lease(Arc<AtomicBool>);
  impl Drop for Lease {
    fn drop(&mut self) {
      self.0.store(true, Ordering::Release);
    }
  }
  let released = Arc::new(AtomicBool::new(false));
  let hardware = Hardware::start();
  let client = hardware.client();
  let mut opening = Box::pin(client.open(Lease(released.clone())));
  assert!(
    opening
      .as_mut()
      .poll(&mut Context::from_waker(Waker::noop()))
      .is_pending()
  );
  // A later request/close is a public FIFO barrier: the first open has
  // finished, but its future deliberately has not received the newly
  // created engine.
  client.open(()).await.unwrap().close().await.unwrap();
  assert!(!released.load(Ordering::Acquire));
  drop(opening);
  tokio::time::timeout(std::time::Duration::from_secs(1), async {
    while !released.load(Ordering::Acquire) {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  hardware.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_advance_applies_ordered_reset_batch_and_returns_all_replies()
 {
  use crate::protocol::command::Command;
  use crate::protocol::{Event, Message, Reply, VERSION};
  let hardware = Hardware::start();
  let mut engine = hardware.client().open(()).await.unwrap();
  for epoch in 1..=16 {
    engine
      .submit(
        Command::try_from(Message {
          version: VERSION,
          kind: Event::Reset as u32,
          epoch,
          ..Default::default()
        })
        .unwrap(),
        None,
      )
      .unwrap();
    assert!(engine.take_reply().is_none());
  }
  assert!(!engine.advance().await.unwrap());
  // Reset invalidates pending replies from preceding epochs.
  let reply = engine.take_reply().unwrap();
  assert_eq!((reply.kind, reply.epoch), (Reply::Stopped as u32, 16));
  assert!(engine.take_reply().is_none());
  engine.close().await.unwrap();
  hardware.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_advance_cancellation_preserves_executed_commands_and_replies()
 {
  use std::future::Future;
  use std::task::{Context, Waker};

  use crate::protocol::command::Command;
  use crate::protocol::{Event, Message, Reply, VERSION};

  let hardware = Hardware::start();
  let client = hardware.client();
  let mut engine = client.open(()).await.unwrap();
  let reset = Command::try_from(Message {
    version: VERSION,
    kind: Event::Reset as u32,
    epoch: 7,
    ..Default::default()
  })
  .unwrap();
  engine.submit(reset, None).unwrap();
  let mut advancing = Box::pin(engine.advance());
  assert!(
    advancing
      .as_mut()
      .poll(&mut Context::from_waker(Waker::noop()))
      .is_pending()
  );
  // The FIFO barrier proves execution completed before cancellation;
  // no sleep or knowledge of executor internals is needed.
  client.open(()).await.unwrap().close().await.unwrap();
  drop(advancing);
  assert_eq!(
    engine.submit(reset, None).unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  assert!(!engine.advance().await.unwrap());
  let reply = engine.take_reply().unwrap();
  assert_eq!((reply.kind, reply.epoch), (Reply::Stopped as u32, 7));
  assert!(!engine.advance().await.unwrap());
  assert!(engine.take_reply().is_none());
  engine.submit(reset, None).unwrap();
  assert!(!engine.advance().await.unwrap());
  assert_eq!(engine.take_reply().unwrap().kind, Reply::Stopped as u32);
  engine.close().await.unwrap();
  hardware.close().await.unwrap();
}
