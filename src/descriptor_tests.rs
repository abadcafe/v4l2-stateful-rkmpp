//! module: descriptor_tests; responsibility: shared fd lifetime and
//! identity.
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd};

use crate::descriptor::Lease;
#[test]
fn test_clone_shares_descriptor_until_final_owner_is_released() {
  let (stream, mut peer) =
    std::os::unix::net::UnixStream::pair().unwrap();
  peer.set_nonblocking(true).unwrap();
  let lease = Lease::new(stream.into());
  let copy = lease.clone();
  assert_eq!(lease.as_raw_fd(), copy.as_fd().as_raw_fd());
  drop(lease);
  assert_eq!(
    peer.read(&mut [0]).unwrap_err().kind(),
    std::io::ErrorKind::WouldBlock
  );
  drop(copy);
  assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}
