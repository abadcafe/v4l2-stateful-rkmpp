//! module: shutdown; responsibility: deliver process termination
//! through owned descriptors. operations: install, consume, shutdown,
//! block_worker, stopping, request; entities: Signals, Shutdown. tests:
//! shutdown_tests, tests/service/test_recovery. No asynchronous Rust
//! signal callbacks.
use std::io;
use std::marker::PhantomData;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::notify::Notify;
struct State {
  notify: Notify,
  stopped: AtomicBool,
}
#[derive(Clone)]
pub struct Shutdown(Arc<State>);
impl Shutdown {
  pub fn stopping(&self) -> bool {
    self.0.stopped.load(Ordering::Acquire)
  }

  pub fn request(&self) {
    self.0.stopped.store(true, Ordering::Release);
    let _ = self.0.notify.signal();
  }
}
impl AsFd for Shutdown {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.0.notify.as_fd()
  }
}
pub struct Signals {
  shutdown: Shutdown,
  fd: OwnedFd,
  old_mask: libc::sigset_t,
  _same_thread: PhantomData<Rc<()>>,
}
impl Signals {
  pub fn install() -> io::Result<Self> {
    let notify = Notify::new()?;
    let mask = termination_mask();
    let mut old_mask = unsafe { std::mem::zeroed() };
    let result = unsafe {
      libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old_mask)
    };
    if result != 0 {
      return Err(io::Error::from_raw_os_error(result));
    }
    let raw = unsafe {
      libc::signalfd(-1, &mask, libc::SFD_NONBLOCK | libc::SFD_CLOEXEC)
    };
    if raw < 0 {
      let error = io::Error::last_os_error();
      unsafe {
        libc::pthread_sigmask(
          libc::SIG_SETMASK,
          &old_mask,
          std::ptr::null_mut(),
        );
      }
      return Err(error);
    }
    Ok(Self {
      shutdown: Shutdown(Arc::new(State {
        notify,
        stopped: AtomicBool::new(false),
      })),
      fd: unsafe { OwnedFd::from_raw_fd(raw) },
      old_mask,
      _same_thread: PhantomData,
    })
  }

  pub fn shutdown(&self) -> Shutdown {
    self.shutdown.clone()
  }

  pub fn consume(&self) -> io::Result<()> {
    let mut info =
      std::mem::MaybeUninit::<libc::signalfd_siginfo>::uninit();
    let size = std::mem::size_of::<libc::signalfd_siginfo>();
    let result = unsafe {
      libc::read(self.fd.as_raw_fd(), info.as_mut_ptr().cast(), size)
    };
    if result < 0 {
      return Err(io::Error::last_os_error());
    }
    if result as usize != size {
      return Err(io::Error::other("incomplete signal notification"));
    }
    let info = unsafe { info.assume_init() };
    if info.ssi_signo == libc::SIGTERM as u32
      || info.ssi_signo == libc::SIGINT as u32
    {
      self.shutdown.request();
    }
    Ok(())
  }

  pub fn block_worker() -> io::Result<()> {
    let mask = termination_mask();
    let result = unsafe {
      libc::pthread_sigmask(
        libc::SIG_BLOCK,
        &mask,
        std::ptr::null_mut(),
      )
    };
    if result != 0 {
      return Err(io::Error::from_raw_os_error(result));
    }
    Ok(())
  }
}
impl AsFd for Signals {
  fn as_fd(&self) -> BorrowedFd<'_> {
    self.fd.as_fd()
  }
}
fn termination_mask() -> libc::sigset_t {
  let mut mask = unsafe { std::mem::zeroed() };
  unsafe {
    libc::sigemptyset(&mut mask);
    libc::sigaddset(&mut mask, libc::SIGTERM);
    libc::sigaddset(&mut mask, libc::SIGINT);
  }
  mask
}
impl Drop for Signals {
  fn drop(&mut self) {
    self.shutdown.request();
    while self.consume().is_ok() {}
    // This !Send owner restores the mask on its original thread.
    unsafe {
      libc::pthread_sigmask(
        libc::SIG_SETMASK,
        &self.old_mask,
        std::ptr::null_mut(),
      );
    }
  }
}
