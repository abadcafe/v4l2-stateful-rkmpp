//! module: pool; responsibility: own a Python Pool wire value.
//! operations: typed fields, encoding and bounded record access;
//! entities: Pool; tests: native_tests.
use super::record::record;
use crate::wire as raw;
record!(Pool, raw::Pool; arrays [buffers];
  fields {
    version => set_version: u32 = [version],
    kind => set_kind: u32 = [kind],
    epoch => set_epoch: u64 = [epoch],
    pool_id => set_pool_id: u32 = [pool_id],
    count => set_count: u32 = [count],
    reserved => set_reserved: [u32; 2] = [reserved],
  }
  methods {
    fn descriptor(
      &self,
      index: usize,
    ) -> Option<super::descriptor::Descriptor> {
      self
        .raw
        .buffers
        .get(index)
        .copied()
        .map(|raw| super::descriptor::Descriptor { raw })
    }

    fn set_descriptor(
      &mut self,
      index: usize,
      value: &super::descriptor::Descriptor,
    ) -> bool {
      let Some(slot) = self.raw.buffers.get_mut(index) else {
        return false;
      };
      *slot = value.raw;
      true
    }

    #[staticmethod]
    fn descriptor_offset(index: usize) -> Option<usize> {
      (index < raw::RB_MAX_BUFFERS as usize).then(|| {
        std::mem::offset_of!(raw::Pool, buffers)
          + index * std::mem::size_of::<raw::Descriptor>()
      })
    }

    fn ioctl(
      &mut self,
      py: pyo3::Python<'_>,
      fd: i32,
      request: u64,
    ) -> i32 {
      if request != raw::RB_IOCTL_REGISTER_POOL {
        return -libc::EINVAL;
      }
      py.detach(|| super::control::ioctl(fd, &mut self.raw))
    }

    fn ioctl_readonly(
      &self,
      py: pyo3::Python<'_>,
      fd: i32,
      request: u64,
    ) -> i32 {
      if request != raw::RB_IOCTL_REGISTER_POOL {
        return -libc::EINVAL;
      }
      py.detach(|| super::control::readonly(fd, &self.raw))
    }
  }
);
