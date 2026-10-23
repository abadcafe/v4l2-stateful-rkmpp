//! module: rpc; responsibility: transfer mock ioctl ownership over
//! a private Unix session. operations: intercept, decode, Response
//! bytes/write; entities: Response, Completed; tests: rpc_tests and
//! Python process/descriptor acceptance.
use std::collections::HashSet;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::{io, mem};
mod memory;
#[cfg(test)]
mod memory_tests;

#[derive(Debug)]
pub struct Response {
  bytes: Vec<u8>,
  owners: Vec<OwnedFd>,
}

/// A completed transport transaction still owns every resource. Only
/// the native syscall entrypoint can publish its result into caller
/// memory.
#[derive(Debug)]
pub enum Completed {
  Accepted(OwnedFd),
  Response(Response),
}
impl Response {
  pub fn bytes(&self) -> &[u8] {
    &self.bytes
  }

  /// # Safety
  /// Writable storage at address must permit exclusive byte writes for
  /// the response extent. Inaccessible addresses return EFAULT.
  pub unsafe fn write(self, address: usize) -> io::Result<()> {
    // SAFETY: the caller authorizes writes to any accessible
    // destination.
    unsafe { memory::write(address, self.bytes()) }?;
    // A failed copyout drops every owner. Only a fully published
    // response transfers descriptor ownership to the ioctl caller.
    for owner in self.owners {
      let _ = owner.into_raw_fd();
    }
    Ok(())
  }
}

fn invalid() -> io::Error {
  io::Error::from_raw_os_error(libc::EPROTO)
}

fn integer(raw: &[u8], offset: usize) -> io::Result<u32> {
  let value = raw.get(offset..offset + 4).ok_or_else(invalid)?;
  Ok(u32::from_le_bytes(value.try_into().map_err(|_| invalid())?))
}

pub fn decode(
  raw: &[u8],
  owners: Vec<OwnedFd>,
  expected: usize,
) -> io::Result<Option<Response>> {
  if raw.len() < 32
    || integer(raw, 0)? != 0x324d4252
    || integer(raw, 28)? != 0
  {
    return Err(invalid());
  }
  let kind = integer(raw, 4)?;
  let code =
    u64::from_le_bytes(raw[8..16].try_into().map_err(|_| invalid())?);
  let length = integer(raw, 16)? as usize;
  let count = integer(raw, 20)? as usize;
  let patches = integer(raw, 24)? as usize;
  if count != owners.len() || count > 32 || patches > 64 {
    return Err(invalid());
  }
  if kind == 3 {
    return if code == 0
      && length == 0
      && raw.len() == 32
      && count == 0
      && patches == 0
    {
      Ok(None)
    } else {
      Err(invalid())
    };
  }
  if kind != 2 {
    return Err(invalid());
  }
  if code != 0 {
    return if code <= i32::MAX as u64
      && raw.len() == 32
      && length == 0
      && count == 0
      && patches == 0
    {
      Err(io::Error::from_raw_os_error(code as i32))
    } else {
      Err(invalid())
    };
  }
  let begin = 32 + patches * 8;
  if length != expected || raw.len() != begin + length {
    return Err(invalid());
  }
  let mut body = raw[begin..].to_vec();
  let mut used = HashSet::new();
  let mut offsets = Vec::with_capacity(patches);
  for index in 0..patches {
    let offset = integer(raw, 32 + index * 8)? as usize;
    let owner = integer(raw, 36 + index * 8)? as usize;
    if owner >= owners.len()
      || offset.checked_add(4).is_none_or(|end| end > body.len())
      || offsets
        .iter()
        .any(|old: &usize| offset < *old + 4 && *old < offset + 4)
    {
      return Err(invalid());
    }
    body[offset..offset + 4]
      .copy_from_slice(&owners[owner].as_raw_fd().to_le_bytes());
    used.insert(owner);
    offsets.push(offset);
  }
  if used.len() != owners.len() {
    return Err(invalid());
  }
  Ok(Some(Response { bytes: body, owners }))
}

fn address(path: &[u8]) -> io::Result<(libc::sockaddr_un, u32)> {
  let mut address = libc::sockaddr_un {
    sun_family: libc::AF_UNIX as libc::sa_family_t,
    sun_path: std::array::from_fn(|_| 0),
  };
  if path.is_empty() || path.len() >= address.sun_path.len() {
    return Err(io::Error::from_raw_os_error(libc::ENAMETOOLONG));
  }
  for (output, value) in address.sun_path.iter_mut().zip(path) {
    *output = *value as libc::c_char;
  }
  let length = (mem::offset_of!(libc::sockaddr_un, sun_path)
    + path.len()
    + 1) as u32;
  Ok((address, length))
}

fn connect(path: &[u8]) -> io::Result<OwnedFd> {
  let (address, length) = address(path)?;
  // SAFETY: The syscall returns one new owned Unix descriptor.
  let fd = unsafe {
    libc::socket(
      libc::AF_UNIX,
      libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
      0,
    )
  };
  if fd < 0 {
    return Err(io::Error::last_os_error());
  }
  // SAFETY: The socket above uniquely created this descriptor.
  let owner = unsafe { OwnedFd::from_raw_fd(fd) };
  // SAFETY: address is initialized for its complete supplied extent.
  if unsafe {
    libc::connect(
      owner.as_raw_fd(),
      (&address as *const libc::sockaddr_un).cast(),
      length,
    )
  } < 0
  {
    return Err(io::Error::last_os_error());
  }
  Ok(owner)
}

fn peer_matches(fd: i32, path: &[u8]) -> bool {
  let mut address = libc::sockaddr_un {
    sun_family: 0,
    sun_path: std::array::from_fn(|_| 0),
  };
  let mut length = mem::size_of_val(&address) as u32;
  // SAFETY: The address and length remain writable for getpeername.
  if unsafe {
    libc::getpeername(
      fd,
      (&mut address as *mut libc::sockaddr_un).cast(),
      &mut length,
    )
  } != 0
    || address.sun_family != libc::AF_UNIX as u16
  {
    return false;
  }
  address
    .sun_path
    .iter()
    .take_while(|byte| **byte != 0)
    .map(|byte| byte.to_ne_bytes()[0])
    .eq(path.iter().copied())
}

fn send(fd: i32, command: u64, body: &[u8]) -> io::Result<()> {
  let length = u32::try_from(body.len()).map_err(|_| invalid())?;
  let mut header = [0u8; 32];
  header[..4].copy_from_slice(&0x324d4252u32.to_le_bytes());
  header[4..8].copy_from_slice(&1u32.to_le_bytes());
  header[8..16].copy_from_slice(&command.to_le_bytes());
  header[16..20].copy_from_slice(&length.to_le_bytes());
  let mut vectors = [
    libc::iovec {
      iov_base: header.as_ptr().cast_mut().cast(),
      iov_len: header.len(),
    },
    libc::iovec {
      iov_base: body.as_ptr().cast_mut().cast(),
      iov_len: body.len(),
    },
  ];
  let message = libc::msghdr {
    msg_name: std::ptr::null_mut(),
    msg_namelen: 0,
    msg_iov: vectors.as_mut_ptr(),
    msg_iovlen: vectors.len(),
    msg_control: std::ptr::null_mut(),
    msg_controllen: 0,
    msg_flags: 0,
  };
  let expected = header.len() + body.len();
  loop {
    // SAFETY: sendmsg only reads the live header/body through these
    // initialized vectors. Both extents remain borrowed through the
    // call; SOCK_SEQPACKET sends them as one message and suppresses
    // SIGPIPE. The body is already kernel-checked owned input.
    let result =
      unsafe { libc::sendmsg(fd, &message, libc::MSG_NOSIGNAL) };
    if result >= 0 && result as usize == expected {
      return Ok(());
    }
    let error = io::Error::last_os_error();
    if result < 0 && error.kind() == io::ErrorKind::Interrupted {
      continue;
    }
    return Err(if result < 0 { error } else { invalid() });
  }
}

fn receive(fd: i32, expected: usize) -> io::Result<Response> {
  loop {
    let mut payload = vec![0u8; 32768];
    // SAFETY: CMSG_SPACE computes the control capacity for 32 i32 fds.
    let control_size = unsafe { libc::CMSG_SPACE(32 * 4) } as usize;
    let mut control =
      vec![0usize; control_size.div_ceil(mem::size_of::<usize>())];
    let mut vector = libc::iovec {
      iov_base: payload.as_mut_ptr().cast(),
      iov_len: payload.len(),
    };
    let mut message = libc::msghdr {
      msg_name: std::ptr::null_mut(),
      msg_namelen: 0,
      msg_iov: &mut vector,
      msg_iovlen: 1,
      msg_control: control.as_mut_ptr().cast(),
      msg_controllen: control_size,
      msg_flags: 0,
    };
    // SAFETY: All receive buffers are writable until the call returns.
    let length = unsafe {
      libc::recvmsg(fd, &mut message, libc::MSG_CMSG_CLOEXEC)
    };
    if length < 0 {
      let error = io::Error::last_os_error();
      if error.kind() == io::ErrorKind::Interrupted {
        continue;
      }
      return Err(error);
    }
    let mut owners = Vec::new();
    let mut valid = true;
    // SAFETY: recvmsg populated message's complete control buffer.
    let mut header = unsafe { libc::CMSG_FIRSTHDR(&message) };
    while !header.is_null() {
      // SAFETY: CMSG traversal stays within recvmsg's owned buffer.
      let value = unsafe { &*header };
      if value.cmsg_level == libc::SOL_SOCKET
        && value.cmsg_type == libc::SCM_RIGHTS
      {
        // SAFETY: CMSG_LEN computes the fixed header extent.
        let base = unsafe { libc::CMSG_LEN(0) } as usize;
        if value.cmsg_len < base
          || !(value.cmsg_len - base).is_multiple_of(4)
        {
          valid = false;
        } else {
          // SAFETY: The payload contains the indicated i32 descriptor
          // array.
          let data = unsafe { libc::CMSG_DATA(header) }.cast::<i32>();
          for index in 0..(value.cmsg_len - base) / 4 {
            // SAFETY: index remains within this control message.
            let received = unsafe { data.add(index).read_unaligned() };
            if received < 0 {
              valid = false;
            } else {
              // SAFETY: recvmsg installed each new descriptor with
              // CLOEXEC.
              owners.push(unsafe { OwnedFd::from_raw_fd(received) });
            }
          }
        }
      } else {
        valid = false;
      }
      // SAFETY: The next header is derived from this received message.
      header = unsafe { libc::CMSG_NXTHDR(&message, header) };
    }
    // A zero-length SEQPACKET can still transfer SCM_RIGHTS. Adopt
    // every delivered descriptor before any post-receive rejection.
    if length == 0 {
      return Err(io::Error::from_raw_os_error(libc::EPIPE));
    }
    if !valid
      || message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0
    {
      return Err(invalid());
    }
    payload.truncate(length as usize);
    if let Some(body) = decode(&payload, owners, expected)? {
      return Ok(body);
    }
  }
}

/// Route a bridge ioctl only when its fd identifies this test's
/// endpoint.
///
/// Invalid or inaccessible input addresses are rejected by the kernel.
/// This operation never writes to caller memory or relinquishes
/// ownership.
pub fn intercept(
  fd: i32,
  command: u64,
  argument: usize,
) -> io::Result<Option<Completed>> {
  if command & 0xff00 != 0x5200 {
    return Ok(None);
  }
  let Some(socket) = std::env::var_os("RKMPP_TEST_KERNEL_SOCKET")
  else {
    return Ok(None);
  };
  let Some(control) = std::env::var_os("RKMPP_TEST_KERNEL_CONTROL")
  else {
    return Ok(None);
  };
  if command == 0x5280
    && std::fs::read_link(format!("/proc/self/fd/{fd}"))
      .is_ok_and(|path| path == control)
  {
    let owner = connect(socket.as_bytes())?;
    send(owner.as_raw_fd(), command, &[])?;
    let _ = receive(owner.as_raw_fd(), 0)?;
    return Ok(Some(Completed::Accepted(owner)));
  }
  if !peer_matches(fd, socket.as_bytes()) {
    return Ok(None);
  }
  let size = ((command >> 16) & 0x3fff) as usize;
  if size != 0 && argument == 0 {
    return Err(io::Error::from_raw_os_error(libc::EFAULT));
  }
  let body = memory::read(argument, size)?;
  send(fd, command, &body)?;
  let response = receive(fd, size)?;
  Ok(Some(Completed::Response(response)))
}
