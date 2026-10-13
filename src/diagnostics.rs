//! module: diagnostics; responsibility: own a bounded read-only Unix
//! endpoint. operations: Endpoint open/drop; entities: Endpoint; tests:
//! diagnostics_tests, tests/service/test_diagnostics. The broker
//! outlives this endpoint by borrowing.
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::notify::Notify;
use crate::shutdown::Shutdown;

mod identity;
#[cfg(test)]
mod identity_tests;
mod status;
#[cfg(test)]
mod status_tests;

pub struct Endpoint<'a> {
  path: PathBuf,
  inode: (u64, u64),
  stop: Notify,
  worker: Option<JoinHandle<()>>,
  _broker: BorrowedFd<'a>,
}

impl<'a> Endpoint<'a> {
  pub fn open(
    path: &Path,
    broker: BorrowedFd<'a>,
    shutdown: &Shutdown,
  ) -> io::Result<Self> {
    let status = status::Status::current(broker, shutdown)?;
    let stop = Notify::new()?;
    reclaim(path)?;
    let listener = UnixListener::bind(path)?;
    let metadata = fs::symlink_metadata(path)?;
    let mut owner = Self {
      path: path.to_owned(),
      inode: (metadata.dev(), metadata.ino()),
      stop,
      worker: None,
      _broker: broker,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    listener.set_nonblocking(true)?;
    let wake = owner.stop.clone();
    owner.worker =
      Some(thread::Builder::new().name("rkmpp-status".into()).spawn(
        move || {
          if let Err(error) = run(listener, wake, status) {
            eprintln!("rkmpp diagnostics: {error}");
          }
        },
      )?);
    Ok(owner)
  }
}

impl Drop for Endpoint<'_> {
  fn drop(&mut self) {
    let _ = self.stop.signal();
    if let Some(worker) = self.worker.take()
      && let Err(panic) = worker.join()
    {
      std::panic::resume_unwind(panic);
    }
    if let Ok(metadata) = fs::symlink_metadata(&self.path)
      && (metadata.dev(), metadata.ino()) == self.inode
    {
      let _ = fs::remove_file(&self.path);
    }
  }
}

fn reclaim(path: &Path) -> io::Result<()> {
  let before = match fs::symlink_metadata(path) {
    Ok(value) => value,
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      return Ok(());
    }
    Err(error) => return Err(error),
  };
  if !before.file_type().is_socket() {
    return Err(io::Error::new(
      io::ErrorKind::AlreadyExists,
      "diagnostics path is not a socket",
    ));
  }
  match UnixStream::connect(path) {
    Ok(_) => {
      return Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "diagnostics socket is active",
      ));
    }
    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
    Err(error) => return Err(error),
  }
  let after = fs::symlink_metadata(path)?;
  if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
    return Err(io::Error::new(
      io::ErrorKind::AlreadyExists,
      "diagnostics path changed",
    ));
  }
  fs::remove_file(path)
}

fn run(
  listener: UnixListener,
  stop: Notify,
  status: status::Status,
) -> io::Result<()> {
  loop {
    let mut fds = [
      libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
      libc::pollfd {
        fd: stop.as_fd().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      },
    ];
    if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
      let error = io::Error::last_os_error();
      if error.kind() == io::ErrorKind::Interrupted {
        continue;
      }
      return Err(error);
    }
    if fds[1].revents != 0 {
      return Ok(());
    }
    let (mut client, _) = match listener.accept() {
      Ok(value) => value,
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
        continue;
      }
      Err(error) => return Err(error),
    };
    client.set_read_timeout(Some(Duration::from_millis(200)))?;
    client.set_write_timeout(Some(Duration::from_millis(200)))?;
    let response =
      respond(&mut client, &status).unwrap_or_else(|error| {
        serde_json::to_vec(
          &serde_json::json!({"error": error.to_string()}),
        )
        .expect("serialize error text")
      });
    let _ = client.write_all(&response);
    let _ = client.write_all(b"\n");
  }
}

fn respond(
  client: &mut UnixStream,
  status: &status::Status,
) -> io::Result<Vec<u8>> {
  let mut request = Vec::with_capacity(64);
  let deadline = Instant::now() + Duration::from_millis(200);
  loop {
    let remaining = deadline
      .checked_duration_since(Instant::now())
      .filter(|duration| !duration.is_zero())
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::TimedOut,
          "diagnostics request deadline",
        )
      })?;
    client.set_read_timeout(Some(remaining))?;
    let mut byte = [0u8; 1];
    client.read_exact(&mut byte)?;
    request.push(byte[0]);
    if byte[0] == b'\n' {
      break;
    }
    if request.len() == 64 {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "diagnostics request exceeds bound",
      ));
    }
  }
  if request != b"snapshot\n" {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "unknown diagnostics request",
    ));
  }
  let response = status.snapshot(client.as_fd())?;
  if response.len() >= 65536 {
    return Err(io::Error::other("diagnostics response exceeds bound"));
  }
  Ok(response)
}
