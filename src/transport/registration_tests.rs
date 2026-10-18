//! module: transport::registration_tests; responsibility: define the
//! corresponding public module contract. operations: Rust test
//! entrypoints; entities: none; tests:
//! test_adopt_registered_buffers_transfers_unique_owned_descriptors
//! test_adopt_invalid_response_headers_close_exported_fds
//! test_adopt_negative_and_duplicate_descriptors_close_other_exports
//! test_adopt_undersized_buffer_closes_all_exports
use std::io::Read;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

// Every adopt call below transfers descriptors produced by
// into_raw_fd; malformed-header cases preserve that ownership
// contract. Duplicate wire entries still represent only one
// transferred OS descriptor.
use super::registration::adopt;
use crate::pool::Direction;
use crate::protocol::VERSION;
use crate::protocol::registration::{Descriptor, MAX_BUFFERS, Pool};

fn returned(count: usize) -> Pool {
  let mut pool = Pool {
    version: VERSION,
    kind: Direction::Output as u32,
    epoch: 3,
    pool_id: 7,
    count: count as u32,
    ..Default::default()
  };
  for descriptor in &mut pool.buffers {
    descriptor.fd = -1;
  }
  for descriptor in &mut pool.buffers[..count] {
    let file = std::fs::File::from(
      crate::dma::Buffer::allocate(4096)
        .unwrap()
        .as_fd()
        .try_clone_to_owned()
        .unwrap(),
    );
    *descriptor = Descriptor { fd: file.into_raw_fd(), size: 4096 };
  }
  pool
}

use std::os::fd::AsFd;

fn rejected(count: usize) -> (Pool, Vec<UnixStream>) {
  let mut pool = returned(0);
  pool.count = count as u32;
  let mut peers = Vec::new();
  for descriptor in &mut pool.buffers[..count] {
    let (owner, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    *descriptor = Descriptor { fd: owner.into_raw_fd(), size: 4096 };
    peers.push(peer);
  }
  (pool, peers)
}

fn closed(peers: Vec<UnixStream>) {
  for mut peer in peers {
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
  }
}

#[test]
fn test_adopt_registered_buffers_transfers_unique_owned_descriptors() {
  let wire = returned(MAX_BUFFERS);
  let first = wire.buffers[0].fd;
  let last = wire.buffers[MAX_BUFFERS - 1].fd;
  let pool = unsafe { adopt(wire, Direction::Output, 3, 7) }.unwrap();
  let (direction, id, buffers) = pool.into_parts();
  assert_eq!(
    (direction, id, buffers.len()),
    (Direction::Output, 7, MAX_BUFFERS)
  );
  assert_eq!(buffers[0].as_fd().as_raw_fd(), first);
  assert_eq!(buffers[MAX_BUFFERS - 1].as_fd().as_raw_fd(), last);
  drop(buffers);
}

#[test]
fn test_adopt_invalid_response_headers_close_exported_fds() {
  for field in 0..6 {
    let (mut wire, peers) = rejected(1);
    match field {
      0 => wire.version += 1,
      1 => wire.kind = Direction::Capture as u32,
      2 => wire.epoch += 1,
      3 => wire.pool_id += 1,
      4 => wire.reserved[0] = 1,
      _ => wire.count = MAX_BUFFERS as u32 + 1,
    }
    assert!(unsafe { adopt(wire, Direction::Output, 3, 7) }.is_err());
    closed(peers);
  }
  assert!(
    unsafe {
      adopt(
        Pool {
          version: VERSION,
          kind: 10,
          epoch: 3,
          pool_id: 7,
          ..Default::default()
        },
        Direction::Output,
        3,
        7,
      )
    }
    .is_err()
  );
}

#[test]
fn test_adopt_negative_and_duplicate_descriptors_close_other_exports() {
  let (mut wire, peers) = rejected(3);
  assert_eq!(unsafe { libc::close(wire.buffers[1].fd) }, 0);
  wire.buffers[1].fd = -1;
  assert!(unsafe { adopt(wire, Direction::Output, 3, 7) }.is_err());
  closed(peers);
  let (mut wire, peers) = rejected(2);
  assert_eq!(unsafe { libc::close(wire.buffers[1].fd) }, 0);
  wire.buffers[1].fd = wire.buffers[0].fd;
  assert!(unsafe { adopt(wire, Direction::Output, 3, 7) }.is_err());
  closed(peers);
}

#[test]
fn test_adopt_undersized_buffer_closes_all_exports() {
  let (mut wire, peers) = rejected(2);
  wire.buffers[0].size = 8192;
  assert!(unsafe { adopt(wire, Direction::Output, 3, 7) }.is_err());
  closed(peers);
}
