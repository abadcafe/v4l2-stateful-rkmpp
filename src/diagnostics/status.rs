//! module: diagnostics::status; responsibility: snapshot this process's
//! resources. operations: Status current/snapshot; entities: Control,
//! Identity, Snapshot, Status; tests: status_tests,
//! tests/service/test_diagnostics. No foreign /proc access.
use std::ffi::CStr;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::{fs, io};

use serde::Serialize;

use super::identity::Image;
use crate::shutdown::Shutdown;

#[derive(Serialize)]
struct Control {
  device: u64,
  inode: u64,
  raw_device: u64,
}
#[derive(Serialize)]
struct Identity {
  pid: u32,
  uid: u32,
  start_ticks: u64,
  build_id: String,
  control: Control,
}
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
  Ready,
  Stopping,
}
#[derive(Serialize)]
struct Measurement<'a> {
  protocol: u32,
  identity: &'a Identity,
  state: State,
  cpu_seconds: f64,
}
#[derive(Serialize)]
struct Snapshot<'a> {
  #[serde(flatten)]
  measurement: Measurement<'a>,
  fds: usize,
  threads: usize,
  sessions: Vec<i32>,
}
pub(super) struct Status {
  identity: Identity,
  shutdown: Shutdown,
}

impl Status {
  pub(super) fn current(
    control: BorrowedFd<'_>,
    shutdown: &Shutdown,
  ) -> io::Result<Self> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat writes one complete stat for the live borrowed
    // descriptor.
    if unsafe {
      libc::fstat(control.as_raw_fd(), metadata.as_mut_ptr())
    } != 0
    {
      return Err(io::Error::last_os_error());
    }
    let metadata = unsafe { metadata.assume_init() };
    let raw = fs::read_to_string("/proc/self/stat")?;
    let fields: Vec<&str> = raw
      .rsplit_once(')')
      .ok_or_else(|| io::Error::other("invalid self stat"))?
      .1
      .split_whitespace()
      .collect();
    let start_ticks = fields
      .get(19)
      .ok_or_else(|| io::Error::other("truncated self stat"))?
      .parse::<u64>()
      .map_err(io::Error::other)?;
    Ok(Self {
      identity: Identity {
        pid: std::process::id(),
        uid: unsafe { libc::geteuid() },
        start_ticks,
        build_id: Image::current()?.build_id().to_owned(),
        control: Control {
          device: metadata.st_dev,
          inode: metadata.st_ino,
          raw_device: metadata.st_rdev,
        },
      },
      shutdown: shutdown.clone(),
    })
  }

  pub(super) fn measure(&self) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&self.measurement()?).map_err(io::Error::other)
  }

  fn measurement(&self) -> io::Result<Measurement<'_>> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes this process's user/system times into
    // usage.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) }
      != 0
    {
      return Err(io::Error::last_os_error());
    }
    let usage = unsafe { usage.assume_init() };
    let cpu_seconds = usage.ru_utime.tv_sec as f64
      + usage.ru_stime.tv_sec as f64
      + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64
        / 1_000_000.0;
    Ok(Measurement {
      protocol: 1,
      identity: &self.identity,
      state: if self.shutdown.stopping() {
        State::Stopping
      } else {
        State::Ready
      },
      cpu_seconds,
    })
  }

  pub(super) fn snapshot(
    &self,
    client: BorrowedFd<'_>,
  ) -> io::Result<Vec<u8>> {
    let measurement = self.measurement()?;
    let (fds, sessions) = inventory(client)?;
    let threads = fs::read_dir("/proc/self/task")?
      .collect::<io::Result<Vec<_>>>()?
      .len();
    let snapshot = Snapshot { measurement, fds, threads, sessions };
    serde_json::to_vec(&snapshot).map_err(io::Error::other)
  }
}

fn inventory(client: BorrowedFd<'_>) -> io::Result<(usize, Vec<i32>)> {
  // Own the enumeration fd explicitly, so another application's
  // directory fd is still counted. getdents64 is the public Linux
  // directory-record ABI.
  let directory = fs::File::open("/proc/self/fd")?;
  let mut buffer = [0u8; 8192];
  let mut fds = 0;
  let mut sessions = Vec::new();
  loop {
    // SAFETY: buffer is writable for its entire advertised length.
    let count = unsafe {
      libc::syscall(
        libc::SYS_getdents64,
        directory.as_raw_fd(),
        buffer.as_mut_ptr(),
        buffer.len(),
      )
    };
    if count < 0 {
      let error = io::Error::last_os_error();
      if error.kind() == io::ErrorKind::Interrupted {
        continue;
      }
      return Err(error);
    }
    if count == 0 {
      break;
    }
    let mut offset = 0;
    while offset < count as usize {
      let bytes = &buffer[offset..count as usize];
      if bytes.len() < 20 {
        return Err(io::Error::other("truncated directory record"));
      }
      let length =
        u16::from_ne_bytes(bytes[16..18].try_into().unwrap()) as usize;
      if length < 20 || length > bytes.len() {
        return Err(io::Error::other(
          "invalid directory record length",
        ));
      }
      let name = CStr::from_bytes_until_nul(&bytes[19..length])
        .map_err(io::Error::other)?
        .to_str()
        .map_err(io::Error::other)?;
      offset += length;
      if matches!(name, "." | "..") {
        continue;
      }
      let fd: i32 = name.parse().map_err(io::Error::other)?;
      if fd == client.as_raw_fd() || fd == directory.as_raw_fd() {
        continue;
      }
      let target = match fs::read_link(format!("/proc/self/fd/{fd}")) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
          continue;
        }
        Err(error) => return Err(error),
      };
      fds += 1;
      if target == std::path::Path::new("anon_inode:v4l2-rkmpp-session")
      {
        sessions.push(fd);
      }
    }
  }
  sessions.sort_unstable();
  Ok((fds, sessions))
}
