//! module: execution_tests; responsibility: specify bounded owned
//! execution. operations: tests; entities: none; tests: lane isolation,
//! thread affinity, cancellation before preparation, native panic
//! propagation, shutdown and reclamation under queue backpressure.
use std::cell::Cell;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::time::Duration;

use crate::execution::{Limits, Pool};

fn limits(workers: usize, backlog: usize) -> Limits {
  Limits {
    workers: NonZeroUsize::new(workers).unwrap(),
    backlog: NonZeroUsize::new(backlog).unwrap(),
  }
}

#[tokio::test(flavor = "current_thread")]
async fn test_call_preserves_non_send_state_and_lane_thread_affinity() {
  let pool = Pool::start(
    "execution-contract",
    limits(2, 4),
    || Rc::new(Cell::new(0u32)),
    |_| {},
  )
  .unwrap();
  let lane = pool.router().lane();
  let first = lane
    .call(|state| {
      state.set(state.get() + 1);
      Ok((std::thread::current().id(), state.get()))
    })
    .await
    .unwrap();
  let second = lane
    .call(|state| {
      state.set(state.get() + 1);
      Ok((std::thread::current().id(), state.get()))
    })
    .await
    .unwrap();
  assert_eq!(first.0, second.0);
  assert_eq!((first.1, second.1), (1, 2));
  assert_ne!(first.0, std::thread::current().id());
  pool.close().await.unwrap();
  assert!(lane.call(|_| Ok(())).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn test_call_blocked_lane_does_not_block_another_lane() {
  let pool =
    Pool::start("execution-isolation", limits(2, 4), || (), |_| {})
      .unwrap();
  let router = pool.router();
  let blocked = router.lane();
  let independent = router.lane();
  let (started, executing) = tokio::sync::oneshot::channel();
  let (release, wait) = std::sync::mpsc::channel();
  let pending = blocked
    .schedule(|| {
      move |_| {
        let _ = started.send(());
        wait.recv().unwrap();
        Ok(())
      }
    })
    .await
    .unwrap();
  executing.await.unwrap();
  let result = tokio::time::timeout(
    Duration::from_secs(1),
    independent.call(|_| Ok(7)),
  )
  .await;
  release.send(()).unwrap();
  pending.await.unwrap();
  assert_eq!(result.unwrap().unwrap(), 7);
  pool.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_schedule_cancellation_during_backpressure_never_prepares_work()
 {
  use std::future::Future;
  use std::task::{Context, Waker};
  let pool =
    Pool::start("execution-budget", limits(1, 2), || (), |_| {})
      .unwrap();
  let lane = pool.router().lane();
  let (started, executing) = tokio::sync::oneshot::channel();
  let (release, wait) = std::sync::mpsc::channel();
  let blocked = lane
    .schedule(|| {
      move |_| {
        let _ = started.send(());
        wait.recv().unwrap();
        Ok(())
      }
    })
    .await
    .unwrap();
  executing.await.unwrap();
  let first = lane.schedule(|| |_| Ok(1)).await.unwrap();
  let second = lane.schedule(|| |_| Ok(2)).await.unwrap();
  let prepared = Cell::new(false);
  let mut scheduling = Box::pin(lane.schedule(|| {
    prepared.set(true);
    |_| Ok(3)
  }));
  assert!(
    scheduling
      .as_mut()
      .poll(&mut Context::from_waker(Waker::noop()))
      .is_pending()
  );
  drop(scheduling);
  assert!(!prepared.get());
  release.send(()).unwrap();
  blocked.await.unwrap();
  assert_eq!(first.await.unwrap(), 1);
  assert_eq!(second.await.unwrap(), 2);
  pool.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_terminated_returns_native_panic_without_hiding_it() {
  let mut pool =
    Pool::start("execution-panic", limits(1, 2), || (), |_| {})
      .unwrap();
  assert!(
    pool
      .router()
      .lane()
      .call::<()>(|_| panic!("executor defect"))
      .await
      .is_err()
  );
  let payload = pool.terminated().await.unwrap_err();
  assert_eq!(*payload.downcast::<&str>().unwrap(), "executor defect");
  pool.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_close_drains_accepted_transactions_and_rejects_new_preparation()
 {
  use std::future::Future;
  use std::task::{Context, Waker};
  let pool =
    Pool::start("execution-close", limits(1, 2), || 0u32, |_| {})
      .unwrap();
  let lane = pool.router().lane();
  let (started, executing) = tokio::sync::oneshot::channel();
  let (release, wait) = std::sync::mpsc::channel();
  let blocked = lane
    .schedule(|| {
      move |_| {
        let _ = started.send(());
        wait.recv().unwrap();
        Ok(())
      }
    })
    .await
    .unwrap();
  executing.await.unwrap();
  let first = lane
    .schedule(|| {
      |state| {
        *state += 1;
        Ok(*state)
      }
    })
    .await
    .unwrap();
  let second = lane
    .schedule(|| {
      |state| {
        *state += 1;
        Ok(*state)
      }
    })
    .await
    .unwrap();
  let mut close = Box::pin(pool.close());
  assert!(
    close
      .as_mut()
      .poll(&mut Context::from_waker(Waker::noop()))
      .is_pending()
  );
  let prepared = Cell::new(false);
  let mut admission = Box::pin(lane.schedule(|| {
    prepared.set(true);
    |_| Ok(())
  }));
  let rejected = matches!(
    admission.as_mut().poll(&mut Context::from_waker(Waker::noop())),
    std::task::Poll::Ready(Err(_))
  );
  drop(admission);
  release.send(()).unwrap();
  close.await.unwrap();
  assert!(rejected);
  assert!(!prepared.get());
  blocked.await.unwrap();
  assert_eq!(first.await.unwrap(), 1);
  assert_eq!(second.await.unwrap(), 2);
  assert!(
    lane
      .schedule(|| {
        prepared.set(true);
        |_| Ok(())
      })
      .await
      .is_err()
  );
  assert!(!prepared.get());
}

#[tokio::test(flavor = "current_thread")]
async fn test_reclaim_full_queue_collects_before_accepted_barrier() {
  let pool = Pool::start(
    "execution-reclaim",
    limits(1, 1),
    Vec::<std::sync::Weak<()>>::new,
    |owners| owners.retain(|owner| owner.strong_count() != 0),
  )
  .unwrap();
  let lane = pool.router().lane();
  let owner = std::sync::Arc::new(());
  let weak = std::sync::Arc::downgrade(&owner);
  lane
    .call(move |owners| {
      owners.push(weak);
      Ok(())
    })
    .await
    .unwrap();
  let (started, executing) = tokio::sync::oneshot::channel();
  let (release, wait) = std::sync::mpsc::channel();
  let blocked = lane
    .schedule(|| {
      move |_| {
        let _ = started.send(());
        wait.recv().unwrap();
        Ok(())
      }
    })
    .await
    .unwrap();
  executing.await.unwrap();
  let barrier =
    lane.schedule(|| |owners| Ok(owners.len())).await.unwrap();
  drop(owner);
  lane.reclaim();
  release.send(()).unwrap();
  blocked.await.unwrap();
  assert_eq!(barrier.await.unwrap(), 0);
  pool.close().await.unwrap();
}
