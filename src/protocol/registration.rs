//! module: protocol::registration; responsibility: resource
//! registration wire contract. operations: Pool, Descriptor,
//! MAX_BUFFERS; entities: Pool, Descriptor; tests: registration_tests,
//! transport registration tests.
pub const MAX_BUFFERS: usize = 32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Descriptor {
  pub fd: i32,
  pub size: u32,
}
impl Default for Descriptor {
  fn default() -> Self {
    Self { fd: -1, size: 0 }
  }
}
#[repr(C)]
#[derive(Default)]
pub struct Pool {
  pub version: u32,
  pub kind: u32,
  pub epoch: u64,
  pub pool_id: u32,
  pub count: u32,
  pub reserved: [u32; 2],
  pub buffers: [Descriptor; MAX_BUFFERS],
}
impl Pool {
  pub(crate) const IOCTL: libc::c_ulong = (3 << 30)
    | ((std::mem::size_of::<Self>() as libc::c_ulong) << 16)
    | (b'R' as libc::c_ulong) << 8
    | 0x85;
}
