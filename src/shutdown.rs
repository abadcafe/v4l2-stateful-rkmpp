//! module: shutdown; responsibility: deliver process termination
//! through owned descriptors. operations: install, consume, shutdown,
//! wait, stopping, request; entities: Signals, Shutdown. tests:
//! shutdown_tests, tests/service/test_recovery. No asynchronous Rust
//! signal callbacks.
use std::io;
use std::marker::PhantomData;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::rc::Rc;
use std::time::Duration;
#[derive(Clone)]
pub struct Shutdown(tokio::sync::watch::Sender<bool>);
impl Shutdown {
  pub fn stopping(&self) -> bool {
    *self.0.borrow()
  }

  pub fn request(&self) {
    self.0.send_replace(true);
  }

  pub async fn cancelled(&self) {
    let mut receiver = self.0.subscribe();
    let _ = receiver.wait_for(|stopped| *stopped).await;
  }
}
pub struct Signals {
  shutdown: Shutdown,
  fd: OwnedFd,
  ready: std::cell::OnceCell<crate::readiness::Readable>,
  old_mask: libc::sigset_t,
  _same_thread: PhantomData<Rc<()>>,
}
impl Signals {
  pub fn install() -> io::Result<Self> {
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
      shutdown: Shutdown(tokio::sync::watch::channel(false).0),
      fd: unsafe { OwnedFd::from_raw_fd(raw) },
      ready: std::cell::OnceCell::new(),
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

  /// Wait without consuming broker readiness; termination interrupts
  /// backoff.
  pub async fn wait(&self, duration: Duration) -> io::Result<()> {
    if self.ready.get().is_none() {
      let ready = crate::readiness::Readable::new(
        self.as_fd().try_clone_to_owned()?,
      )?;
      let _ = self.ready.set(ready);
    }
    let ready = self.ready.get().unwrap();
    tokio::select! {
      result = ready.wait() => { result?; self.consume() },
      () = self.shutdown.cancelled() => Ok(()),
      () = tokio::time::sleep(duration) => Ok(()),
    }
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
