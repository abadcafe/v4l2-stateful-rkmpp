//! module: shutdown_tests; responsibility: define the corresponding
//! public module contract. operations: Rust test entrypoints; entities:
//! none; tests:
//! test_install_empty_signal_queue_is_nonblocking
//! test_request_clones_observe_stop_and_readable_notification
//! test_consume_sigterm_requests_shutdown
//! test_consume_sigint_requests_shutdown
//! test_drop_restores_original_thread_signal_mask
//! test_block_worker_blocks_termination_on_owned_thread
use std::os::fd::{AsFd, AsRawFd};
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

#[test]
fn test_request_clones_observe_stop_and_readable_notification() {
  let signals = Signals::install().unwrap();
  let stop = signals.shutdown();
  let other = stop.clone();
  let thread = std::thread::spawn(move || other.request());
  thread.join().unwrap();
  assert!(stop.stopping());
  let mut fd = libc::pollfd {
    fd: stop.as_fd().as_raw_fd(),
    events: libc::POLLIN,
    revents: 0,
  };
  assert_eq!(
    unsafe {
      libc::poll(&mut fd, 1, Duration::from_secs(1).as_millis() as i32)
    },
    1
  );
  assert_ne!(fd.revents & libc::POLLIN, 0);
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

#[test]
fn test_block_worker_blocks_termination_on_owned_thread() {
  std::thread::spawn(|| {
    Signals::block_worker().unwrap();
    let mut mask = unsafe { std::mem::zeroed() };
    assert_eq!(
      unsafe {
        libc::pthread_sigmask(
          libc::SIG_SETMASK,
          std::ptr::null(),
          &mut mask,
        )
      },
      0
    );
    for signal in [libc::SIGINT, libc::SIGTERM] {
      assert_eq!(unsafe { libc::sigismember(&mask, signal) }, 1);
    }
  })
  .join()
  .unwrap();
}
