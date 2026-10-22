//! module: shutdown_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_install_empty_signal_queue_is_nonblocking
//! test_request_clones_observe_stop_and_readable_notification
//! test_consume_sigterm_requests_shutdown
//! test_consume_sigint_requests_shutdown
//! test_drop_restores_original_thread_signal_mask
//! test_wait_installed_signal_requires_no_spare_descriptor
//! test_wait_install_before_runtime_retains_pending_signal
use std::time::Duration;

use crate::shutdown::Signals;

#[test]
fn test_install_empty_signal_queue_is_nonblocking() {
  let signals = Signals::install().unwrap();
  assert!(!signals.shutdown().stopping());
  assert_eq!(
    signals.consume().unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
}

#[tokio::test(flavor = "current_thread")]
async fn test_request_clones_observe_stop_without_lost_wakeups() {
  let signals = Signals::install().unwrap();
  let stop = signals.shutdown();
  let other = stop.clone();
  let waiting = tokio::spawn(async move {
    other.cancelled().await;
  });
  stop.request();
  waiting.await.unwrap();
  assert!(stop.stopping());
  stop.cancelled().await;
  stop.request();
  assert!(stop.stopping());
}

#[test]
fn test_consume_sigterm_requests_shutdown() {
  let signals = Signals::install().unwrap();
  assert_eq!(
    unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGTERM) },
    0
  );
  signals.consume().unwrap();
  assert!(signals.shutdown().stopping());
}

#[test]
fn test_consume_sigint_requests_shutdown() {
  let signals = Signals::install().unwrap();
  assert_eq!(
    unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGINT) },
    0
  );
  signals.consume().unwrap();
  assert!(signals.shutdown().stopping());
}

#[test]
fn test_drop_restores_original_thread_signal_mask() {
  fn mask() -> libc::sigset_t {
    let mut value = unsafe { std::mem::zeroed() };
    assert_eq!(
      unsafe {
        libc::pthread_sigmask(
          libc::SIG_SETMASK,
          std::ptr::null(),
          &mut value,
        )
      },
      0
    );
    value
  }
  let original = mask();
  let signals = Signals::install().unwrap();
  for signal in [libc::SIGINT, libc::SIGTERM] {
    assert_eq!(unsafe { libc::sigismember(&mask(), signal) }, 1);
  }
  let shutdown = signals.shutdown();
  drop(signals);
  assert!(shutdown.stopping());
  for signal in [libc::SIGINT, libc::SIGTERM] {
    assert_eq!(
      unsafe { libc::sigismember(&original, signal) },
      unsafe { libc::sigismember(&mask(), signal) }
    );
  }
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_timeout_and_cross_thread_shutdown() {
  let signals = Signals::install().unwrap();
  let started = std::time::Instant::now();
  signals.wait(Duration::from_millis(20)).await.unwrap();
  assert!(started.elapsed() >= Duration::from_millis(20));
  let stop = signals.shutdown();
  let thread = std::thread::spawn(move || stop.request());
  let began = std::time::Instant::now();
  signals.wait(Duration::from_secs(5)).await.unwrap();
  thread.join().unwrap();
  assert!(began.elapsed() < Duration::from_secs(1));
  assert!(signals.shutdown().stopping());
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_pending_termination_interrupts_backoff() {
  let signals = Signals::install().unwrap();
  assert_eq!(
    unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGTERM) },
    0
  );
  signals.wait(Duration::from_secs(5)).await.unwrap();
  assert!(signals.shutdown().stopping());
}

#[test]
fn test_wait_install_before_runtime_retains_pending_signal() {
  let signals = Signals::install().unwrap();
  assert_eq!(
    unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGINT) },
    0
  );
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  runtime.block_on(signals.wait(Duration::from_secs(1))).unwrap();
  assert!(signals.shutdown().stopping());
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_installed_signal_requires_no_spare_descriptor() {
  const CHILD: &str = "RKMPP_SIGNAL_PRESSURE_CHILD";
  if std::env::var_os(CHILD).is_none() {
    // Isolate the process-wide resource limit from parallel module
    // tests.
    let output = std::process::Command::new(
      std::env::current_exe().unwrap(),
    )
    .args([
      "--exact",
      "shutdown_tests::test_wait_installed_signal_requires_no_spare_descriptor",
      "--nocapture",
    ])
    .env(CHILD, "1")
    .output()
    .unwrap();
    assert!(
      output.status.success(),
      "{}{}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    );
    return;
  }
  let signals = Signals::install().unwrap();
  let mut original = std::mem::MaybeUninit::<libc::rlimit>::uninit();
  // SAFETY: original is writable storage for the complete limit.
  assert_eq!(
    unsafe {
      libc::getrlimit(libc::RLIMIT_NOFILE, original.as_mut_ptr())
    },
    0
  );
  // SAFETY: getrlimit successfully initialized original.
  let original = unsafe { original.assume_init() };
  let limit = libc::rlimit {
    rlim_cur: original.rlim_cur.min(64),
    rlim_max: original.rlim_max,
  };
  // SAFETY: limit is initialized; only this isolated process is
  // affected.
  assert_eq!(
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
    0
  );
  let mut files = Vec::new();
  let exhausted = loop {
    match std::fs::File::open("/dev/null") {
      Ok(file) => files.push(file),
      Err(error) => break error,
    }
  };
  assert_eq!(exhausted.raw_os_error(), Some(libc::EMFILE));
  assert_eq!(
    unsafe { libc::pthread_kill(libc::pthread_self(), libc::SIGTERM) },
    0
  );
  let result = signals.wait(Duration::from_secs(1)).await;
  drop(files);
  // SAFETY: restore the successful getrlimit result before assertions.
  assert_eq!(
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &original) },
    0
  );
  result.unwrap();
  assert!(signals.shutdown().stopping());
}
