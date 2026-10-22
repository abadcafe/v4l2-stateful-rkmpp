//! module: batch; responsibility: own a Python Batch wire value.
//! operations: typed fields, encoding and bounded record access;
//! entities: Batch; tests: native_tests.
use pyo3::Bound;
use pyo3::types::{PyAnyMethods, PyInt};

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
    fn event(
      &self,
      index: &Bound<'_, PyInt>,
    ) -> Option<super::message::Message> {
      self
        .raw
        .events
        .get(index.extract::<usize>().ok()?)
        .copied()
        .map(|raw| super::message::Message { raw })
    }

    fn reply(
      &self,
      index: &Bound<'_, PyInt>,
    ) -> Option<super::message::Message> {
      self
        .raw
        .replies
        .get(index.extract::<usize>().ok()?)
        .copied()
        .map(|raw| super::message::Message { raw })
    }

    fn set_event(
      &mut self,
      index: &Bound<'_, PyInt>,
      value: &super::message::Message,
    ) -> bool {
      let Ok(index) = index.extract::<usize>() else {
        return false;
      };
      let Some(slot) = self.raw.events.get_mut(index) else {
        return false;
      };
      *slot = value.raw;
      true
    }

    fn set_reply(
      &mut self,
      index: &Bound<'_, PyInt>,
      value: &super::message::Message,
    ) -> bool {
      let Ok(index) = index.extract::<usize>() else {
        return false;
      };
      let Some(slot) = self.raw.replies.get_mut(index) else {
        return false;
      };
      *slot = value.raw;
      true
    }

    fn result(&self, index: &Bound<'_, PyInt>) -> Option<i32> {
      self.raw.results
        .get(index.extract::<usize>().ok()?)
        .copied()
    }

    fn results(&self, count: &Bound<'_, PyInt>) -> Option<Vec<i32>> {
      self.raw.results
        .get(..count.extract::<usize>().ok()?)
        .map(<[i32]>::to_vec)
    }

    fn set_result(
      &mut self,
      index: &Bound<'_, PyInt>,
      value: i32,
    ) -> bool {
      let Ok(index) = index.extract::<usize>() else {
        return false;
      };
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
