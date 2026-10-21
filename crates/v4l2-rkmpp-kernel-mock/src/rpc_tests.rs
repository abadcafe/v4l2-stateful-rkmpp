//! module: rpc_tests; responsibility: specify mock reply ownership.
//! operations: tests; entities: none; all tests use module public API.
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::rpc::decode;

#[test]
fn test_intercept_empty_descriptor_message_closes_received_owners() {
  const CHILD: &str = "RKMPP_EMPTY_MESSAGE_CHILD";
  if std::env::var_os(CHILD).is_some() {
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let before = count();
    let mut payload = 0u32;
    // SAFETY: payload is exclusively owned, writable ioctl storage.
    let result = unsafe {
      super::rpc::intercept(
        0,
        (3 << 30) | (4 << 16) | 0x5284,
        (&mut payload as *mut u32) as usize,
      )
    };
    assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EPIPE));
    assert_eq!(count(), before, "empty message leaked SCM_RIGHTS");
    return;
  }
  let mut pair = [-1; 2];
  // SAFETY: pair provides space for both newly created descriptors.
  assert_eq!(
    unsafe {
      libc::socketpair(
        libc::AF_UNIX,
        libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
        0,
        pair.as_mut_ptr(),
      )
    },
    0
  );
  // SAFETY: socketpair transferred unique ownership of both fds.
  let [receiver, sender] =
    pair.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) });
  let child = std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "rpc_tests::test_intercept_empty_descriptor_message_closes_received_owners", "--nocapture"])
    .env(CHILD, "1")
    .env("RKMPP_TEST_KERNEL_SOCKET", "")
    .env("RKMPP_TEST_KERNEL_CONTROL", "/unused-test-control")
    .stdin(std::process::Stdio::from(receiver))
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn().unwrap();
  let (owner, _peer) = descriptor();
  // SAFETY: CMSG_SPACE sizes aligned storage for one descriptor.
  let size = unsafe { libc::CMSG_SPACE(4) } as usize;
  let mut control =
    vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
  // SAFETY: zero is a valid empty msghdr; no payload is sent.
  let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
  message.msg_control = control.as_mut_ptr().cast();
  message.msg_controllen = size;
  // SAFETY: the aligned control buffer contains one complete header
  // and descriptor; sendmsg borrows them only for this call.
  unsafe {
    let header = libc::CMSG_FIRSTHDR(&message);
    (*header).cmsg_level = libc::SOL_SOCKET;
    (*header).cmsg_type = libc::SCM_RIGHTS;
    (*header).cmsg_len = libc::CMSG_LEN(4) as usize;
    libc::CMSG_DATA(header)
      .cast::<i32>()
      .write_unaligned(owner.as_raw_fd());
    assert_eq!(
      libc::sendmsg(sender.as_raw_fd(), &message, libc::MSG_NOSIGNAL),
      0
    );
  }
  let output = child.wait_with_output().unwrap();
  assert!(
    output.status.success(),
    "{}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
}

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
    decode(&raw, Vec::new(), 4)
      .unwrap()
      .map(|response| response.bytes().to_vec()),
    Some(vec![1, 2, 3, 4])
  );
  let mut wake = packet(0, &[], 0, &[]);
  wake[4..8].copy_from_slice(&3u32.to_le_bytes());
  assert!(decode(&wake, Vec::new(), 4).unwrap().is_none());
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
  let response = decode(&raw, vec![owner], 8).unwrap().unwrap();
  let mut body = [0u8; 8];
  // SAFETY: body is exclusively owned writable response storage.
  unsafe { response.write(body.as_mut_ptr() as usize) }.unwrap();
  assert_eq!(i32::from_le_bytes(body[..4].try_into().unwrap()), fd);
  assert_eq!(i32::from_le_bytes(body[4..].try_into().unwrap()), fd);
  // SAFETY: Successful decode transferred this descriptor to the
  // payload.
  drop(unsafe { OwnedFd::from_raw_fd(fd) });
  closed(peer);
}

#[test]
fn test_decode_unpublished_or_failed_copyout_closes_received_descriptors()
 {
  for address in [None, Some(1usize)] {
    let (owner, peer) = descriptor();
    let response =
      decode(&packet(0, &[0; 4], 1, &[(0, 0)]), vec![owner], 4)
        .unwrap()
        .unwrap();
    if let Some(address) = address {
      // SAFETY: an inaccessible address cannot alias Rust storage.
      assert_eq!(
        unsafe { response.write(address) }.unwrap_err().raw_os_error(),
        Some(libc::EFAULT)
      );
    } else {
      drop(response);
    }
    closed(peer);
  }
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
