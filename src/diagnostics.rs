//! module: diagnostics; responsibility: own a bounded read-only Unix
//! endpoint. operations: Endpoint open/drop; entities: Endpoint; tests:
//! diagnostics_tests, tests/service/test_diagnostics. The broker
//! outlives this endpoint by borrowing. Resource pressure at accept
//! backs off without failing the service or existing decode sessions.
use std::num::NonZeroUsize;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

use crate::execution::{Client, Limits, Pool};
use crate::shutdown::Shutdown;

mod identity;
#[cfg(test)]
mod identity_tests;
mod status;
#[cfg(test)]
mod status_tests;

const ADMISSION_BACKOFF: Duration = Duration::from_millis(100);

pub struct Endpoint<'a> {
  path: PathBuf,
  inode: (u64, u64),
  task: Option<JoinHandle<()>>,
  worker: Option<Pool<()>>,
  _broker: BorrowedFd<'a>,
}

impl<'a> Endpoint<'a> {
  pub async fn open(
    path: &Path,
    broker: BorrowedFd<'a>,
    shutdown: &Shutdown,
  ) -> io::Result<Self> {
    let status = status::Status::current(broker, shutdown)?;
    reclaim(path).await?;
    // A timeout cannot preempt synchronous procfs calls on the runtime.
    // Bound both running and queued inventory independently of clients.
    let worker = Pool::start(
      "rkmpp-status",
      Limits {
        workers: NonZeroUsize::new(1).unwrap(),
        backlog: NonZeroUsize::new(1).unwrap(),
      },
      || (),
      |_| {},
    )?;
    let client = worker.router().lane();
    let listener = UnixListener::bind(path)?;
    let metadata = fs::symlink_metadata(path)?;
    let mut owner = Self {
      path: path.to_owned(),
      inode: (metadata.dev(), metadata.ino()),
      task: None,
      worker: Some(worker),
      _broker: broker,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    owner.task = Some(tokio::spawn(async move {
      if let Err(error) = run(listener, Arc::new(status), client).await
      {
        eprintln!("rkmpp diagnostics: {error}");
      }
    }));
    Ok(owner)
  }
}

impl Endpoint<'_> {
  pub async fn terminated(&mut self) -> std::thread::Result<()> {
    tokio::select! {
      result = self.task.as_mut().unwrap() => {
        self.task = None;
        match result {
          Err(error) if error.is_panic() => Err(error.into_panic()),
          _ => Ok(()),
        }
      },
      result = self.worker.as_mut().unwrap().terminated() => {
        result
      },
    }
  }

  pub async fn close(mut self) -> std::thread::Result<()> {
    let result = if let Some(task) = self.task.take() {
      task.abort();
      match task.await {
        Err(error) if error.is_panic() => Err(error.into_panic()),
        _ => Ok(()),
      }
    } else {
      Ok(())
    };
    let joined = match self.worker.take() {
      Some(worker) => worker.close().await,
      None => Ok(()),
    };
    result.and(joined)
  }
}

impl Drop for Endpoint<'_> {
  fn drop(&mut self) {
    if let Some(task) = self.task.take() {
      task.abort();
    }
    if let Ok(metadata) = fs::symlink_metadata(&self.path)
      && (metadata.dev(), metadata.ino()) == self.inode
    {
      let _ = fs::remove_file(&self.path);
    }
  }
}

async fn reclaim(path: &Path) -> io::Result<()> {
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
  match tokio::time::timeout(
    Duration::from_millis(200),
    UnixStream::connect(path),
  )
  .await
  .map_err(|_| {
    io::Error::new(
      io::ErrorKind::AddrInUse,
      "diagnostics socket connection timed out",
    )
  })? {
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

async fn run(
  listener: UnixListener,
  status: Arc<status::Status>,
  worker: Client<()>,
) -> io::Result<()> {
  loop {
    let (client, _) = match listener.accept().await {
      Ok(connection) => connection,
      Err(error)
        if matches!(
          error.raw_os_error(),
          Some(
            libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOMEM
          )
        ) =>
      {
        // A readable listener stays readable while accept cannot
        // allocate a descriptor. Yield on a timer rather than
        // spinning, and keep this endpoint alive for recovery.
        eprintln!("rkmpp diagnostics: admission pressure: {error}");
        tokio::time::sleep(ADMISSION_BACKOFF).await;
        continue;
      }
      Err(error)
        if matches!(
          error.kind(),
          io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted
        ) =>
      {
        continue;
      }
      Err(error) => return Err(error),
    };
    // A queued inventory operation retains the actual client
    // descriptor, even if the async deadline expires. It must never
    // scan using a raw descriptor number that another client could
    // have reused.
    let (read, mut write) = client.into_split();
    let response = match tokio::time::timeout(
      Duration::from_millis(200),
      respond(read, &status, &worker),
    )
    .await
    {
      Ok(result) => result,
      Err(_) => Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "diagnostics request deadline",
      )),
    }
    .unwrap_or_else(|error| {
      serde_json::to_vec(
        &serde_json::json!({"error": error.to_string()}),
      )
      .expect("serialize error text")
    });
    let _ = tokio::time::timeout(Duration::from_millis(200), async {
      write.write_all(&response).await?;
      write.write_all(b"\n").await
    })
    .await;
  }
}
async fn respond(
  mut client: tokio::net::unix::OwnedReadHalf,
  status: &Arc<status::Status>,
  worker: &Client<()>,
) -> io::Result<Vec<u8>> {
  let mut request = [0u8; 64];
  let mut length = 0;
  let end = loop {
    let count = client.read(&mut request[length..]).await?;
    if count == 0 {
      return Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "incomplete diagnostics request",
      ));
    }
    length += count;
    if let Some(end) =
      request[..length].iter().position(|&byte| byte == b'\n')
    {
      break end + 1;
    }
    if length == request.len() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "diagnostics request exceeds bound",
      ));
    }
  };
  let status = status.clone();
  let response = match &request[..end] {
    b"snapshot\n" => {
      worker
        .call(move |_| status.snapshot(client.as_ref().as_fd()))
        .await?
    }
    b"measure\n" => {
      worker
        .call(move |_| {
          // Retain the stream until this accepted operation has
          // completed.
          let response = status.measure();
          drop(client);
          response
        })
        .await?
    }
    _ => {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "unknown diagnostics request",
      ));
    }
  };
  if response.len() >= 65536 {
    return Err(io::Error::other("diagnostics response exceeds bound"));
  }
  Ok(response)
}
