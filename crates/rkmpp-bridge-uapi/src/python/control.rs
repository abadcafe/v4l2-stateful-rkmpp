//! module: control; responsibility: perform bounded synchronous UAPI
//! transfers. operations: ioctl, readonly; entities: none; tests:
//! native_tests and kernel copyout cases.
use bytemuck::Pod;
pub(crate) trait Wire: Pod {
  const COMMAND: u64;
}
impl Wire for crate::wire::Batch {
  const COMMAND: u64 = crate::wire::RB_IOCTL_EXCHANGE;
}
impl Wire for crate::wire::Pool {
  const COMMAND: u64 = crate::wire::RB_IOCTL_REGISTER_POOL;
}

fn status(value: libc::c_int) -> i32 {
  if value < 0 {
    -std::io::Error::last_os_error().raw_os_error().unwrap()
  } else {
    value
  }
}
pub(crate) fn ioctl<T: Wire>(fd: i32, payload: &mut T) -> i32 {
  let bytes = bytemuck::bytes_of_mut(payload);
  // SAFETY: the generated POD storage is initialized, aligned and
  // exclusively borrowed for this synchronous call. No pointer
  // escapes into Python.
  status(unsafe {
    libc::ioctl(fd, T::COMMAND as libc::c_ulong, bytes.as_mut_ptr())
  })
}
pub(crate) fn readonly<T: Wire>(fd: i32, payload: &T) -> i32 {
  let mut mapping =
    match memmap2::MmapMut::map_anon(std::mem::size_of::<T>()) {
      Ok(mapping) => mapping,
      Err(error) => return -error.raw_os_error().unwrap(),
    };
  mapping.copy_from_slice(bytemuck::bytes_of(payload));
  let mapping = match mapping.make_read_only() {
    Ok(mapping) => mapping,
    Err(error) => return -error.raw_os_error().unwrap(),
  };
  // SAFETY: the owned mapping stays readable and alive through the
  // syscall. Kernel copyout fails with EFAULT; it cannot mutate this
  // read-only mapping.
  status(unsafe {
    libc::ioctl(fd, T::COMMAND as libc::c_ulong, mapping.as_ptr())
  })
}
