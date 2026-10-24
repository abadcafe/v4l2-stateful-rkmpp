//! module: rkmpp::packet; responsibility: own reusable packets over
//! persistent input maps. operations: Input new/prepare,
//! InputAccess bytes/packet/finish, Packet end/consumed/handle;
//! entities: Input, Packet, OwnedPacket, InputAccess, Storage, Cache,
//! Payload. tests:
//! packet_tests, mpp_tests, V4L2 input ownership.
use std::cell::RefCell;
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;
use std::rc::Rc;

use dma_buf::memory::Buffer;
use dma_buf::memory::persistent::{InputMap, ReadAccess};

use super::ffi::{
  mpp_packet_deinit, mpp_packet_get_buffer, mpp_packet_get_length,
  mpp_packet_get_segment_nb, mpp_packet_has_meta, mpp_packet_init,
  mpp_packet_set_data, mpp_packet_set_dts, mpp_packet_set_eos,
  mpp_packet_set_flag, mpp_packet_set_length, mpp_packet_set_pos,
  mpp_packet_set_pts, mpp_packet_set_size,
};
#[derive(Default)]
struct Cache(RefCell<Vec<NonNull<c_void>>>);
impl Drop for Cache {
  fn drop(&mut self) {
    for handle in self.0.get_mut().drain(..) {
      let mut raw = handle.as_ptr();
      unsafe {
        mpp_packet_deinit(&mut raw);
      }
    }
  }
}
// These variants make the storage lifetime inseparable from its SDK
// handle; registered mappings are not decoder packets.
enum Storage {
  Borrowed { cache: Rc<Cache>, _lease: Rc<ReadAccess> },
  Owned { _bytes: Vec<u8> },
}
pub struct Packet {
  owner: Rc<OwnedPacket>,
}
struct OwnedPacket {
  handle: NonNull<c_void>,
  storage: Storage,
}
pub struct Input {
  // Deinitialize cached SDK packets before releasing their mapping.
  cache: Rc<Cache>,
  input: Rc<InputMap>,
}
impl Input {
  pub fn new(buffer: Buffer) -> io::Result<Self> {
    Ok(Self {
      input: Rc::new(InputMap::new(buffer)?),
      cache: Rc::new(Cache::default()),
    })
  }

  pub fn prepare(
    &self,
    offset: usize,
    length: usize,
  ) -> io::Result<InputAccess> {
    Ok(InputAccess {
      cache: self.cache.clone(),
      access: Rc::new(self.input.read(offset, length)?),
    })
  }
}
pub struct InputAccess {
  cache: Rc<Cache>,
  access: Rc<ReadAccess>,
}
impl InputAccess {
  pub fn bytes(&self) -> &[u8] {
    self.access.bytes()
  }

  /// Submit a checked input range, borrowing ordinary frames and
  /// assembling prefixed frames into one padded allocation. Only
  /// borrowed packets retain the input CPU read lease.
  pub fn packet(
    &self,
    payload: Payload,
    identity: u64,
    dts: i64,
  ) -> io::Result<Packet> {
    let range = match &payload {
      Payload::Input(range) | Payload::Prefixed { range, .. } => range,
    };
    if range.start >= range.end || range.end > self.bytes().len() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "task range outside input lease",
      ));
    }
    if let Payload::Prefixed { prefix, range } = payload {
      return Packet::assemble(
        prefix,
        &self.bytes()[range],
        identity,
        dts,
      );
    }
    let Payload::Input(range) = payload else { unreachable!() };
    let pointer = self.bytes()[range.clone()].as_ptr().cast_mut();
    let handle = if let Some(handle) = self.cache.0.borrow_mut().pop() {
      handle
    } else {
      let mut handle = std::ptr::null_mut();
      let status = unsafe {
        mpp_packet_init(&mut handle, pointer.cast(), range.len())
      };
      if status != 0 {
        return Err(io::Error::other(format!(
          "MPP task packet: {status}"
        )));
      }
      NonNull::new(handle)
        .ok_or_else(|| io::Error::other("null MPP task packet"))?
    };
    unsafe {
      mpp_packet_set_data(handle.as_ptr(), pointer.cast());
      mpp_packet_set_size(handle.as_ptr(), range.len());
      mpp_packet_set_pos(handle.as_ptr(), pointer.cast());
      mpp_packet_set_length(handle.as_ptr(), range.len());
      mpp_packet_set_flag(handle.as_ptr(), 0);
    }
    unsafe {
      mpp_packet_set_pts(handle.as_ptr(), identity as i64);
      mpp_packet_set_dts(handle.as_ptr(), dts);
    }
    Ok(Packet {
      owner: Rc::new(OwnedPacket {
        handle,
        storage: Storage::Borrowed {
          cache: self.cache.clone(),
          _lease: self.access.clone(),
        },
      }),
    })
  }

  pub fn finish(self) -> io::Result<()> {
    Rc::try_unwrap(self.access)
      .map_err(|_| {
        io::Error::other("input still has live coded-frame tasks")
      })?
      .finish()
  }
}
impl Packet {
  pub(super) fn end() -> io::Result<Self> {
    let packet = Self::assemble(Vec::new(), &[0], 0, 0)?;
    unsafe {
      mpp_packet_set_length(packet.handle(), 0);
      mpp_packet_set_eos(packet.handle());
    }
    Ok(packet)
  }

  fn assemble(
    mut bytes: Vec<u8>,
    picture: &[u8],
    timestamp: u64,
    dts: i64,
  ) -> io::Result<Self> {
    let length = bytes
      .len()
      .checked_add(picture.len())
      .filter(|&size| size != 0 && size <= MAX_PACKET_SIZE)
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::InvalidInput,
          "invalid access unit size",
        )
      })?;
    // Reserve the SDK padding before copying any coded picture bytes.
    // Growth can move metadata, but never an already assembled frame.
    bytes.reserve_exact(picture.len() + 64);
    bytes.extend_from_slice(picture);
    bytes.resize(length + 64, 0);
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
      mpp_packet_init(&mut handle, bytes.as_mut_ptr().cast(), length)
    };
    if status != 0 {
      return Err(io::Error::other(format!(
        "MPP packet init: {status}"
      )));
    }
    let handle = NonNull::new(handle)
      .ok_or_else(|| io::Error::other("null MPP packet"))?;
    unsafe {
      mpp_packet_set_pts(handle.as_ptr(), timestamp as i64);
      // Preserve the caller's independent DTS metadata channel.
      mpp_packet_set_dts(handle.as_ptr(), dts);
    }
    Ok(Self {
      owner: Rc::new(OwnedPacket {
        handle,
        storage: Storage::Owned { _bytes: bytes },
      }),
    })
  }

  pub(super) fn retain(&self) -> Self {
    Self { owner: self.owner.clone() }
  }

  pub(super) fn consumed(&self) -> bool {
    unsafe { mpp_packet_get_length(self.handle()) == 0 }
  }

  pub(super) fn handle(&self) -> *mut c_void {
    self.owner.handle.as_ptr()
  }
}
impl Drop for OwnedPacket {
  fn drop(&mut self) {
    let mut handle = self.handle.as_ptr();
    if let Storage::Borrowed { cache, .. } = &self.storage {
      // SDK-added metadata and segments have no public reset operation.
      // Only completely resettable packet objects may enter the free
      // list.
      let reusable = unsafe {
        mpp_packet_has_meta(handle) == 0
          && mpp_packet_get_buffer(handle).is_null()
          && mpp_packet_get_segment_nb(handle) == 0
      };
      if reusable && cache.0.borrow().len() < 32 {
        cache.0.borrow_mut().push(self.handle);
        return;
      }
    }
    unsafe {
      mpp_packet_deinit(&mut handle);
    }
  }
}

/// Compressed packet storage relative to a retained input access.
#[derive(Debug)]
pub enum Payload {
  Input(std::ops::Range<usize>),
  Prefixed { prefix: Vec<u8>, range: std::ops::Range<usize> },
}
/// Bound owned packet assembly, including metadata, to 16 MiB.
pub const MAX_PACKET_SIZE: usize = 16 * 1024 * 1024;
