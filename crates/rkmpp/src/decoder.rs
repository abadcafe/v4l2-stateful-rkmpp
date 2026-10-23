//! module: rkmpp::decoder; responsibility: manage a nonblocking decoder
//! and its callbacks. operations: new, packet/frame, output
//! configuration, reset/close; entities: Decoder, Submission.
//! tests: decoder_tests, mpp_tests and public V4L2 codec/pixel/recovery
//! tests.
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

use dma_buf::memory::Buffer;

use super::api::Api;
use super::frame::Frame;
use super::group::Group;
use super::packet::Packet;
use super::reservation::Reservation;
type Handle = *mut c_void;
use super::ffi::{
  mpp_create, mpp_dec_cfg_deinit, mpp_dec_cfg_init,
  mpp_dec_cfg_set_ptr, mpp_destroy, mpp_init,
};
struct Config(Handle);
impl Drop for Config {
  fn drop(&mut self) {
    unsafe {
      mpp_dec_cfg_deinit(self.0);
    }
  }
}
/// Admission and execution progress are independent: a synchronous call
/// can finish an internally staged task without consuming the offered
/// packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submission {
  Blocked,
  Progress,
  Consumed,
}
pub struct Decoder {
  context: Option<NonNull<c_void>>,
  api: Api,
  group: Option<Group>,
  retired: Vec<Group>,
  callback: Box<dyn Fn() -> io::Result<()> + Send + Sync>,
  parked: bool,
  info_wait: bool,
  frames: std::collections::VecDeque<Frame>,
  publish_on_completion: bool,
  completion_packet: Option<Packet>,
  publication_pending: bool,
}
unsafe extern "C" fn notify_ready<F: Fn() -> io::Result<()>>(
  context: Handle,
  _mpp: Handle,
  _command: i32,
  _argument: Handle,
) -> i32 {
  // SAFETY: callback is a pinned Box owned by Decoder; mpp_destroy
  // joins its callbacks before release.
  let notification = unsafe { &*context.cast::<F>() };
  if notification().is_ok() { 0 } else { -1 }
}
fn check(operation: &str, status: i32) -> io::Result<()> {
  if status == 0 {
    Ok(())
  } else {
    Err(io::Error::other(format!("MPP {operation}: {status}")))
  }
}
impl Decoder {
  pub fn new<F: Fn() -> io::Result<()> + Send + Sync + 'static>(
    codec: u32,
    notify: F,
  ) -> io::Result<Self> {
    let (coding, split) = match codec.to_le_bytes() {
      [b'H', b'2', b'6', b'4'] => {
        (super::ffi::MPP_VIDEO_CODING_AVC, 0u32)
      }
      [b'H', b'E', b'V', b'C'] => {
        (super::ffi::MPP_VIDEO_CODING_HEVC, 0)
      }
      [b'V', b'P', b'8', b'0'] => (super::ffi::MPP_VIDEO_CODING_VP8, 0),
      [b'V', b'P', b'9', b'0'] => (super::ffi::MPP_VIDEO_CODING_VP9, 0),
      [b'A', b'V', b'0', b'1'] => (super::ffi::MPP_VIDEO_CODING_AV1, 0),
      _ => {
        return Err(io::Error::new(
          io::ErrorKind::InvalidInput,
          "unsupported video codec",
        ));
      }
    };
    let mut context = std::ptr::null_mut();
    let mut api = std::ptr::null_mut();
    check("create", unsafe { mpp_create(&mut context, &mut api) })?;
    let context = NonNull::new(context)
      .ok_or_else(|| io::Error::other("null MPP context"))?;
    let Some(api) = NonNull::new(api) else {
      unsafe {
        mpp_destroy(context.as_ptr());
      }
      return Err(io::Error::other("null MPP API"));
    };
    // SAFETY: mpp_create publishes an initialized size-prefixed ABI;
    // librockchip_mpp stays linked throughout this owner's lifetime.
    let api = match unsafe { Api::read(api) } {
      Ok(api) => api,
      Err(error) => {
        unsafe {
          mpp_destroy(context.as_ptr());
        }
        return Err(error);
      }
    };
    let mut decoder = Self {
      context: Some(context),
      api,
      group: None,
      parked: false,
      info_wait: false,
      frames: std::collections::VecDeque::new(),
      publish_on_completion: coding == super::ffi::MPP_VIDEO_CODING_AV1,
      completion_packet: None,
      publication_pending: false,
      retired: Vec::new(),
      callback: Box::new(notify),
    };
    decoder.control(
      super::ffi::MPP_DEC_SET_PARSER_SPLIT_MODE,
      (&split as *const u32).cast_mut().cast(),
    )?;
    let zero = 0u32;
    decoder.control(
      super::ffi::MPP_DEC_SET_ENABLE_DEINTERLACE,
      (&zero as *const u32).cast_mut().cast(),
    )?;
    let one = 1u32;
    decoder.control(
      super::ffi::MPP_SET_DISABLE_THREAD,
      (&one as *const u32).cast_mut().cast(),
    )?;
    decoder.control(
      super::ffi::MPP_DEC_SET_IMMEDIATE_OUT,
      (&one as *const u32).cast_mut().cast(),
    )?;
    check("init", unsafe {
      mpp_init(context.as_ptr(), super::ffi::MPP_CTX_DEC, coding)
    })?;
    let timeout = 0i64;
    decoder.control(
      super::ffi::MPP_SET_OUTPUT_TIMEOUT,
      (&timeout as *const i64).cast_mut().cast(),
    )?;
    decoder.control(
      super::ffi::MPP_SET_INPUT_TIMEOUT,
      (&timeout as *const i64).cast_mut().cast(),
    )?;
    let mut config = std::ptr::null_mut();
    check("config init", unsafe { mpp_dec_cfg_init(&mut config) })?;
    let config = Config(config);
    decoder.control(super::ffi::MPP_DEC_GET_CFG, config.0)?;
    let callback = notify_ready::<F> as *const () as Handle;
    let context = (&*decoder.callback
      as *const (dyn Fn() -> io::Result<()> + Send + Sync))
      .cast::<c_void>()
      .cast_mut();
    for (key, value) in
      [(c"cb:frm_rdy_cb", callback), (c"cb:frm_rdy_ctx", context)]
    {
      check("callback config", unsafe {
        mpp_dec_cfg_set_ptr(config.0, key.as_ptr(), value)
      })?;
    }
    decoder.control(super::ffi::MPP_DEC_SET_CFG, config.0)?;
    Ok(decoder)
  }

  fn control(
    &mut self,
    command: u32,
    parameter: Handle,
  ) -> io::Result<()> {
    // SAFETY: all callers are this owner; the context remains live and
    // parameters have the specific MPP control command's
    // representation.
    check("control", unsafe {
      (self.api.control)(
        self.context.unwrap().as_ptr(),
        command,
        parameter,
      )
    })
  }

  pub fn packet(&mut self, packet: &Packet) -> io::Result<Submission> {
    // Output publication and registered storage bound admission.
    // Exhaustion waits for a real lease release; it never grows the
    // pool.
    if self.parked
      || self.publication_pending
      || !self.frames.is_empty()
      || self.info_wait
    {
      return Ok(Submission::Blocked);
    }
    if !self.can_decode() {
      return Ok(Submission::Blocked);
    }
    self.completion_packet = None;
    self.decode(packet.handle())?;
    if packet.consumed() {
      self.publication_pending = self.publish_on_completion;
      return Ok(Submission::Consumed);
    }
    Ok(Submission::Progress)
  }

  fn can_decode(&self) -> bool {
    !self.info_wait && self.group.as_ref().is_none_or(Group::available)
  }

  fn decode(&mut self, packet: Handle) -> io::Result<()> {
    let mut raw = std::ptr::null_mut();
    check("decode", unsafe {
      (self.api.decode)(
        self.context.unwrap().as_ptr(),
        packet,
        &mut raw,
      )
    })?;
    if let Some(frame) = unsafe { Frame::from_raw(raw) } {
      self.enqueue(frame)?;
    }
    // decode() may return an older queued picture before the
    // info-change notification from this task. Drain the public
    // output queue before deciding whether the hardware task is
    // parked or complete.
    loop {
      let mut queued = std::ptr::null_mut();
      super::status::frame(unsafe {
        (self.api.get)(self.context.unwrap().as_ptr(), &mut queued)
      })?;
      let Some(frame) = (unsafe { Frame::from_raw(queued) }) else {
        break;
      };
      self.enqueue(frame)?;
    }
    self.parked = self.info_wait;
    Ok(())
  }

  fn enqueue(&mut self, frame: Frame) -> io::Result<()> {
    let info = frame.info();
    // AV1's public EOS operation is an internal publication fence,
    // not a V4L2 end-of-stream event. Never expose its empty marker.
    if self.publish_on_completion
      && info.eos
      && !info.info_change
      && !info.failed
      && frame.plane()?.is_none()
    {
      return Ok(());
    }
    self.info_wait |= info.info_change;
    self.frames.push_back(frame);
    Ok(())
  }

  /// No staged hardware task, delayed publication or public frame.
  /// The caller still owns codec-level presentation obligations.
  pub fn idle(&self) -> bool {
    !self.parked && !self.publication_pending && self.frames.is_empty()
  }

  fn publish(&mut self) -> io::Result<()> {
    // MPP AV1 queues show_existing_frame without a hardware task.
    // Its verified reference-preserving EOS path publishes that queue.
    // Do this at each task completion, not only STOP: a bounded client
    // may await the image before submitting the next hardware task.
    // H.26x EOS flush destroys reference state; VP9 EOS also fails
    // continuation pixel tests. Neither uses this AV1 adapter policy.
    self.completion_packet = Some(Packet::end()?);
    let handle = self.completion_packet.as_ref().unwrap().handle();
    self.decode(handle)?;
    self.publication_pending = false;
    Ok(())
  }

  pub fn frame(&mut self) -> io::Result<Option<Frame>> {
    self.retired.retain(Group::in_use);
    if self.frames.is_empty() && self.parked && self.can_decode() {
      self.decode(std::ptr::null_mut())?;
    }
    if self.publication_pending && !self.parked {
      self.publish()?;
    }
    Ok(self.frames.pop_front())
  }

  pub fn configure_output(&mut self, ten_bit: bool) -> io::Result<()> {
    let group = Group::new(ten_bit)?;
    self
      .control(super::ffi::MPP_DEC_SET_EXT_BUF_GROUP, group.handle())?;
    if let Some(previous) = self.group.replace(group) {
      self.retired.push(previous);
      self.retired.last_mut().unwrap().retire()?;
    }
    self.retired.retain(Group::in_use);
    Ok(())
  }

  /// Retain the external group's leases when reusing the same CAPTURE
  /// storage, including codec references whose display leases ended.
  pub fn reuse_output(
    &mut self,
    ten_bit: bool,
  ) -> io::Result<Option<Vec<Reservation>>> {
    if !ten_bit && self.group.as_ref().is_some_and(Group::external) {
      return self.group.as_ref().unwrap().pin_capture().map(Some);
    }
    self.configure_output(ten_bit)?;
    Ok(None)
  }

  pub fn has_output(&self) -> bool {
    self.group.is_some()
  }

  pub fn import_capture(
    &mut self,
    buffers: &[(usize, Buffer)],
    capacity: usize,
  ) -> io::Result<()> {
    self
      .group
      .as_mut()
      .ok_or_else(|| io::Error::other("decoder has no output group"))?
      .commit(buffers, capacity)
  }

  pub fn reserve_capture(
    &mut self,
    size: usize,
  ) -> io::Result<Option<Reservation>> {
    let group = self.group.as_ref().ok_or_else(|| {
      io::Error::other("decoder has no capture group")
    })?;
    group.reserve_capture(size)
  }

  pub fn ready(&mut self) -> io::Result<()> {
    let group = self
      .group
      .as_ref()
      .ok_or_else(|| io::Error::other("decoder has no output group"))?
      .handle();
    self.control(super::ffi::MPP_DEC_SET_EXT_BUF_GROUP, group)?;
    self.control(
      super::ffi::MPP_DEC_SET_INFO_CHANGE_READY,
      std::ptr::null_mut(),
    )?;
    self.info_wait = false;
    Ok(())
  }

  pub fn reset(&mut self) -> io::Result<()> {
    self.frames.clear();
    self.parked = false;
    self.info_wait = false;
    check("reset", unsafe {
      (self.api.reset)(self.context.unwrap().as_ptr())
    })?;
    self.completion_packet = None;
    self.publication_pending = false;
    Ok(())
  }

  pub fn close(mut self) -> io::Result<()> {
    self.dispose()
  }

  fn dispose(&mut self) -> io::Result<()> {
    let Some(context) = self.context.take() else {
      return Ok(());
    };
    self.frames.clear();
    let reset = unsafe { (self.api.reset)(context.as_ptr()) };
    let destroy = unsafe { mpp_destroy(context.as_ptr()) };
    self.group = None;
    self.retired.clear();
    check("reset", reset).and(check("destroy", destroy))
  }
}
impl Drop for Decoder {
  fn drop(&mut self) {
    let _ = self.dispose();
  }
}
