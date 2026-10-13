//! module: rpc_tests; responsibility: specify mock reply ownership.
//! operations: tests; entities: none; all tests use module public API.
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::rpc::decode;

fn packet(
  code: u64,
  body: &[u8],
  count: u32,
  patches: &[(u32, u32)],
) -> Vec<u8> {
  let mut raw = Vec::from(0x324d4252u32.to_le_bytes());
  raw.extend(2u32.to_le_bytes());
  raw.extend(code.to_le_bytes());
  raw.extend((body.len() as u32).to_le_bytes());
  raw.extend(count.to_le_bytes());
  raw.extend((patches.len() as u32).to_le_bytes());
  raw.extend(0u32.to_le_bytes());
  for (offset, index) in patches {
    raw.extend(offset.to_le_bytes());
    raw.extend(index.to_le_bytes());
  }
  raw.extend(body);
  raw
}

fn descriptor() -> (OwnedFd, UnixStream) {
  let (owner, peer) = UnixStream::pair().unwrap();
  peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
  (owner.into(), peer)
}

fn closed(mut peer: UnixStream) {
  assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn test_decode_payload_is_exact_and_wake_is_separate() {
  let raw = packet(0, &[1, 2, 3, 4], 0, &[]);
  assert_eq!(
    decode(&raw, Vec::new(), 4).unwrap(),
    Some(vec![1, 2, 3, 4])
  );
  let mut wake = packet(0, &[], 0, &[]);
  wake[4..8].copy_from_slice(&3u32.to_le_bytes());
  assert_eq!(decode(&wake, Vec::new(), 4).unwrap(), None);
}

#[test]
fn test_decode_error_preserves_kernel_errno() {
  let error =
    decode(&packet(libc::ESTALE as u64, &[], 0, &[]), Vec::new(), 4)
      .unwrap_err();
  assert_eq!(error.raw_os_error(), Some(libc::ESTALE));
}

#[test]
fn test_decode_transfers_cloexec_fd_and_supports_alias_patch() {
  let (owner, peer) = descriptor();
  let fd = owner.as_raw_fd();
  let raw = packet(0, &[0; 8], 1, &[(0, 0), (4, 0)]);
  let body = decode(&raw, vec![owner], 8).unwrap().unwrap();
  assert_eq!(i32::from_le_bytes(body[..4].try_into().unwrap()), fd);
  assert_eq!(i32::from_le_bytes(body[4..].try_into().unwrap()), fd);
  // SAFETY: Successful decode transferred this descriptor to the
  // payload.
  drop(unsafe { OwnedFd::from_raw_fd(fd) });
  closed(peer);
}

#[test]
fn test_decode_invalid_wire_or_patch_closes_every_received_fd() {
  for (offset, index, count, size) in
    [(5, 0, 1, 8), (0, 1, 1, 8), (0, 0, 2, 8), (0, 0, 1, 4)]
  {
    let (owner, peer) = descriptor();
    let raw = packet(0, &[0; 8], count, &[(offset, index)]);
    assert!(decode(&raw, vec![owner], size).is_err());
    closed(peer);
  }
}

#[test]
fn test_decode_truncated_wrong_magic_or_reserved_is_rejected() {
  for (offset, value) in [(0, 0), (4, 9), (28, 1)] {
    let mut raw = packet(0, &[], 0, &[]);
    raw[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    assert!(decode(&raw, Vec::new(), 0).is_err());
  }
  assert!(decode(&[0; 31], Vec::new(), 0).is_err());
}

#[test]
fn test_decode_wake_requires_zero_length_and_no_ownership() {
  let mut wake = packet(0, &[], 0, &[]);
  wake[4..8].copy_from_slice(&3u32.to_le_bytes());
  wake[16..20].copy_from_slice(&1u32.to_le_bytes());
  assert!(decode(&wake, Vec::new(), 0).is_err());
}

#[test]
fn test_decode_unused_or_overlapping_descriptor_patches_close_all_owners()
 {
  for patches in [vec![(0, 0)], vec![(0, 0), (2, 1)]] {
    let (first, first_peer) = descriptor();
    let (second, second_peer) = descriptor();
    let owners = vec![first, second];
    assert!(
      decode(&packet(0, &[0; 8], 2, &patches), owners, 8).is_err()
    );
    closed(first_peer);
    closed(second_peer);
  }
}
