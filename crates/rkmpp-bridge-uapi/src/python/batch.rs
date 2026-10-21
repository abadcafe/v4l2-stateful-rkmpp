//! module: batch; responsibility: own a Python Batch wire value.
//! operations: typed fields, encoding and bounded record access;
//! entities: Batch; tests: native_tests.
use super::record::record;
use crate::wire as raw;
record!(Batch, raw::Batch; arrays [replies, events, results];
  fields {
    version => set_version: u32 = [version],
    reply_count => set_reply_count: u32 = [reply_count],
    event_count => set_event_count: u32 = [event_count],
    reserved => set_reserved: u32 = [reserved],
  }
  methods {
    fn event(&self, index: usize) -> Option<super::message::Message> {
      self
        .raw
        .events
        .get(index)
        .copied()
        .map(|raw| super::message::Message { raw })
    }

    fn reply(&self, index: usize) -> Option<super::message::Message> {
      self
        .raw
        .replies
        .get(index)
        .copied()
        .map(|raw| super::message::Message { raw })
    }

    fn set_event(
      &mut self,
      index: usize,
      value: &super::message::Message,
    ) -> bool {
      let Some(slot) = self.raw.events.get_mut(index) else {
        return false;
      };
      *slot = value.raw;
      true
    }

    fn set_reply(
      &mut self,
      index: usize,
      value: &super::message::Message,
    ) -> bool {
      let Some(slot) = self.raw.replies.get_mut(index) else {
        return false;
      };
      *slot = value.raw;
      true
    }

    fn result(&self, index: usize) -> Option<i32> {
      self.raw.results.get(index).copied()
    }

    fn results(&self, count: usize) -> Option<Vec<i32>> {
      self.raw.results.get(..count).map(<[i32]>::to_vec)
    }

    fn set_result(&mut self, index: usize, value: i32) -> bool {
      let Some(slot) = self.raw.results.get_mut(index) else {
        return false;
      };
      *slot = value;
      true
    }

    fn ioctl(
      &mut self,
      py: pyo3::Python<'_>,
      fd: i32,
      request: u64,
    ) -> i32 {
      if request != raw::RB_IOCTL_EXCHANGE {
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
      if request != raw::RB_IOCTL_EXCHANGE {
        return -libc::EINVAL;
      }
      py.detach(|| super::control::readonly(fd, &self.raw))
    }
  }
);
