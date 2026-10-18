//! module: fence; responsibility: retain and observe an external GPU
//! completion. operations: new, ready, lease; entities: Fence; tests:
//! fence_tests.
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

use crate::descriptor::Lease;
#[derive(Debug)]
pub struct Fence {
  descriptor: Option<Lease>,
  submitted: Instant,
}
impl Fence {
  pub fn new(descriptor: Option<OwnedFd>) -> Self {
    Self {
      descriptor: descriptor.map(Lease::new),
      submitted: Instant::now(),
    }
  }

  pub fn lease(&self) -> Option<Lease> {
    self.descriptor.clone()
  }

  pub fn ready(&self) -> io::Result<bool> {
    let Some(fd) = &self.descriptor else {
      return Ok(true);
    };
    let mut poll = libc::pollfd {
      fd: fd.as_raw_fd(),
      events: libc::POLLIN,
      revents: 0,
    };
    loop {
      // SAFETY: one live descriptor, nonblocking readiness query.
      if unsafe { libc::poll(&mut poll, 1, 0) } >= 0 {
        break;
      }
      let error = io::Error::last_os_error();
      if error.kind() != io::ErrorKind::Interrupted {
        return Err(error);
      }
    }
    if poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
      return Err(io::Error::other("GPU completion failed"));
    }
    if poll.revents & libc::POLLIN != 0 && completion(fd.as_raw_fd())? {
      return Ok(true);
    }
    if self.submitted.elapsed() >= Duration::from_secs(2) {
      return Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "GPU completion timed out",
      ));
    }
    Ok(false)
  }
}

// Linux sync_file_info.status distinguishes a successfully signaled
// fence from one signaled with an error. poll readiness alone does not.
fn completion(fd: libc::c_int) -> io::Result<bool> {
  #[repr(C)]
  #[derive(Default)]
  struct Info {
    name: [u8; 32],
    status: i32,
    flags: u32,
    fences: u32,
    pad: u32,
    details: u64,
  }
  let mut info = Info::default();
  const COMMAND: libc::c_ulong = (3 << 30)
    | ((std::mem::size_of::<Info>() as libc::c_ulong) << 16)
    | ((b'>' as libc::c_ulong) << 8)
    | 4;
  loop {
    // SAFETY: complete UAPI record, num_fences=0 requests summary only.
    if unsafe { libc::ioctl(fd, COMMAND, &mut info) } == 0 {
      break;
    }
    let error = io::Error::last_os_error();
    if error.kind() != io::ErrorKind::Interrupted {
      return Err(error);
    }
    info = Info::default();
  }
  match info.status {
    0 => Ok(false),
    1 => Ok(true),
    negative if negative < 0 => Err(io::Error::from_raw_os_error(
      negative.checked_neg().unwrap_or(libc::EIO),
    )),
    _ => Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid sync-file completion status",
    )),
  }
}
