//! module: converter_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! Lease, PanickingInput; tests:
//! test_new_empty_converter_has_no_completion
//! test_submit_completions_preserve_fifo_tokens_and_input_ownership
//! test_submit_full_retains_rejected_job_and_completion_releases_capacity
//! test_submit_invalid_extent_reports_error_then_recovers
//! test_drop_joins_pending_work_and_releases_every_input_lease
//! test_drop_propagates_worker_programming_panic
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::converter::{Completion, Converter, Input, Job};
use crate::dma::Buffer;
use crate::image::{Image, Plane};
use crate::notify::Notify;
fn image() -> Image {
  Image::new(Buffer::allocate(3072).unwrap(), 64, 32)
}
fn take<T: Input>(
  converter: &mut Converter<T>,
  wake: &Notify,
) -> Completion<T> {
  let deadline = Instant::now() + Duration::from_secs(5);
  loop {
    if let Some(completion) = converter.take().unwrap() {
      return completion;
    }
    assert!(
      Instant::now() < deadline,
      "conversion completion deadline"
    );
    wake.wait(Duration::from_millis(50)).unwrap();
    wake.drain().unwrap();
  }
}
#[test]
fn test_new_empty_converter_has_no_completion() {
  let mut converter =
    Converter::<Image>::new(Notify::new().unwrap()).unwrap();
  assert!(converter.take().unwrap().is_none());
}
#[test]
fn test_submit_completions_preserve_fifo_tokens_and_input_ownership() {
  let wake = Notify::new().unwrap();
  let mut converter = Converter::new(wake.clone()).unwrap();
  for token in 0..3 {
    converter
      .submit(Job::new(token, (32, 32), image(), image()))
      .unwrap();
  }
  for token in 0..3 {
    let (actual, input, status) =
      take(&mut converter, &wake).into_parts();
    assert_eq!(actual, token);
    status.unwrap();
    assert_eq!(
      (input.plane().stride(), input.plane().rows()),
      (64, 32)
    );
  }
  assert!(converter.take().unwrap().is_none());
}
#[test]
fn test_submit_full_retains_rejected_job_and_completion_releases_capacity()
 {
  let wake = Notify::new().unwrap();
  let mut converter = Converter::new(wake.clone()).unwrap();
  for token in 0..3 {
    converter
      .submit(Job::new(token, (32, 32), image(), image()))
      .unwrap();
  }
  let rejected = converter
    .submit(Job::new(99, (32, 32), image(), image()))
    .unwrap_err();
  assert!(rejected.is_full());
  assert!(format!("{rejected:?}").contains("full: true"));
  let held = rejected.into_job().into_input();
  assert_eq!(held.plane().rows(), 32);
  take(&mut converter, &wake).into_parts().2.unwrap();
  converter.submit(Job::new(4, (32, 32), held, image())).unwrap();
  for _ in 0..3 {
    take(&mut converter, &wake).into_parts().2.unwrap();
  }
}
#[test]
fn test_submit_invalid_extent_reports_error_then_recovers() {
  let wake = Notify::new().unwrap();
  let mut converter = Converter::new(wake.clone()).unwrap();
  converter.submit(Job::new(8, (0, 32), image(), image())).unwrap();
  let (token, _input, status) =
    take(&mut converter, &wake).into_parts();
  assert_eq!(token, 8);
  assert_eq!(
    status.unwrap_err().kind(),
    std::io::ErrorKind::InvalidInput
  );
  converter.submit(Job::new(9, (32, 32), image(), image())).unwrap();
  let (token, _, status) = take(&mut converter, &wake).into_parts();
  assert_eq!(token, 9);
  status.unwrap();
}
struct Lease {
  image: Image,
  dropped: Arc<AtomicUsize>,
}
impl Input for Lease {
  fn plane(&self) -> Plane<'_> {
    self.image.plane()
  }
}
impl Drop for Lease {
  fn drop(&mut self) {
    self.dropped.fetch_add(1, Ordering::SeqCst);
  }
}
#[test]
fn test_drop_joins_pending_work_and_releases_every_input_lease() {
  let count = Arc::new(AtomicUsize::new(0));
  let mut converter = Converter::new(Notify::new().unwrap()).unwrap();
  for token in 0..3 {
    converter
      .submit(Job::new(
        token,
        (32, 32),
        Lease { image: image(), dropped: count.clone() },
        image(),
      ))
      .unwrap();
  }
  drop(converter);
  assert_eq!(count.load(Ordering::SeqCst), 3);
}
struct PanickingInput(Arc<AtomicBool>);
impl Input for PanickingInput {
  fn plane(&self) -> Plane<'_> {
    self.0.store(true, Ordering::SeqCst);
    panic!("programming fault in conversion input")
  }
}
#[test]
fn test_drop_propagates_worker_programming_panic() {
  let entered = Arc::new(AtomicBool::new(false));
  let mut converter = Converter::new(Notify::new().unwrap()).unwrap();
  converter
    .submit(Job::new(
      0,
      (32, 32),
      PanickingInput(entered.clone()),
      image(),
    ))
    .unwrap();
  let deadline = Instant::now() + Duration::from_secs(5);
  while !entered.load(Ordering::SeqCst) {
    assert!(Instant::now() < deadline);
    std::thread::yield_now();
  }
  assert!(
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(
      converter
    )))
    .is_err()
  );
}

#[test]
fn test_take_observes_worker_panic_without_drop_and_rejects_more_work()
{
  let wake = Notify::new().unwrap();
  let mut converter = Converter::new(wake.clone()).unwrap();
  converter
    .submit(Job::new(
      0,
      (32, 32),
      PanickingInput(Arc::new(AtomicBool::new(false))),
      image(),
    ))
    .unwrap();
  assert!(wake.wait(Duration::from_secs(5)).unwrap());
  assert!(
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      converter.take()
    }))
    .is_err()
  );
  let rejected = converter
    .submit(Job::new(
      1,
      (32, 32),
      PanickingInput(Arc::new(AtomicBool::new(false))),
      image(),
    ))
    .unwrap_err();
  assert!(!rejected.is_full());
  assert!(converter.take().is_err());
}

#[test]
fn test_submit_observes_worker_panic_before_admitting_new_job() {
  let wake = Notify::new().unwrap();
  let mut converter = Converter::new(wake.clone()).unwrap();
  converter
    .submit(Job::new(
      0,
      (32, 32),
      PanickingInput(Arc::new(AtomicBool::new(false))),
      image(),
    ))
    .unwrap();
  assert!(wake.wait(Duration::from_secs(5)).unwrap());
  assert!(
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      converter.submit(Job::new(
        1,
        (32, 32),
        PanickingInput(Arc::new(AtomicBool::new(false))),
        image(),
      ))
    }))
    .is_err()
  );
}
