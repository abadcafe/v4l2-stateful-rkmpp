//! module: fence; responsibility: retain and observe an external GPU
//! completion. operations: new, ready, wait, lease; entities: Fence;
//! tests: fence_tests.
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

  pub fn wait(&self) -> io::Result<()> {
    let Some(fd) = &self.descriptor else {
      return Ok(());
    };
    loop {
      if signaled(fd.as_raw_fd(), poll(fd, -1)?)? {
        return Ok(());
      }
    }
  }

  pub fn ready(&self) -> io::Result<bool> {
    let Some(fd) = &self.descriptor else {
      return Ok(true);
    };
    if signaled(fd.as_raw_fd(), poll(fd, 0)?)? {
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

fn poll(fd: &Lease, timeout: i32) -> io::Result<libc::c_short> {
  let mut descriptor = libc::pollfd {
    fd: fd.as_raw_fd(),
    events: libc::POLLIN,
    revents: 0,
  };
  loop {
    // SAFETY: the lease retains the descriptor for this synchronous
    // query, and descriptor is one fully initialized writable record.
    if unsafe { libc::poll(&mut descriptor, 1, timeout) } >= 0 {
      return Ok(descriptor.revents);
    }
    let error = io::Error::last_os_error();
    if error.kind() != io::ErrorKind::Interrupted {
      return Err(error);
    }
  }
}

fn signaled(
  fd: libc::c_int,
  events: libc::c_short,
) -> io::Result<bool> {
  if events & libc::POLLNVAL != 0 {
    return Err(io::Error::other("invalid GPU completion descriptor"));
  }
  if events & (libc::POLLIN | libc::POLLERR) != 0 {
    return completion(fd);
  }
  if events & libc::POLLHUP != 0 {
    return Err(io::Error::new(
      io::ErrorKind::BrokenPipe,
      "GPU completion descriptor closed",
    ));
  }
  Ok(false)
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
