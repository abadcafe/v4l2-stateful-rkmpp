//! module: mpp::decoder; responsibility: manage a nonblocking decoder
//! and its callbacks. operations: new, packet/frame, output
//! configuration, reset/close; entities: Decoder. tests: decoder_tests,
//! mpp_tests and public V4L2 codec/pixel/recovery tests.
use std::ffi::{c_char, c_void};
use std::io;
use std::ptr::NonNull;

use super::frame::Frame;
use super::group::Group;
use super::packet::Packet;
use crate::dma::Buffer;
use crate::notify::Notify;
type Handle = *mut c_void;
#[repr(C)]
struct Api {
  size: u32,
  version: u32,
  decode: usize,
  put: Option<unsafe extern "C" fn(Handle, Handle) -> i32>,
  get: Option<unsafe extern "C" fn(Handle, *mut Handle) -> i32>,
  other: [usize; 9],
  reset: Option<unsafe extern "C" fn(Handle) -> i32>,
  control: Option<unsafe extern "C" fn(Handle, u32, Handle) -> i32>,
  reserved: [u32; 16],
}
#[link(name = "rockchip_mpp")]
unsafe extern "C" {
  fn mpp_create(context: *mut Handle, api: *mut *mut Api) -> i32;
  fn mpp_init(context: Handle, kind: u32, coding: u32) -> i32;
  fn mpp_destroy(context: Handle) -> i32;
  fn mpp_dec_cfg_init(config: *mut Handle) -> i32;
  fn mpp_dec_cfg_deinit(config: Handle) -> i32;
  fn mpp_dec_cfg_set_ptr(
    config: Handle,
    key: *const c_char,
    value: Handle,
  ) -> i32;
}
struct Config(Handle);
impl Drop for Config {
  fn drop(&mut self) {
    unsafe {
      mpp_dec_cfg_deinit(self.0);
    }
  }
}
pub struct Decoder {
  context: Option<NonNull<c_void>>,
  api: NonNull<Api>,
  group: Option<Group>,
  retired: Vec<Group>,
  callback: Box<Notify>,
}
unsafe extern "C" fn notify_ready(
  context: Handle,
  _mpp: Handle,
  _command: i32,
  _argument: Handle,
) -> i32 {
  // SAFETY: callback is a pinned Box owned by Decoder; mpp_destroy
  // joins its callbacks before release.
  let notification = unsafe { &*context.cast::<Notify>() };
  if notification.signal().is_ok() { 0 } else { -1 }
}
fn check(operation: &str, status: i32) -> io::Result<()> {
  if status == 0 {
    Ok(())
  } else {
    Err(io::Error::other(format!("MPP {operation}: {status}")))
  }
}
impl Decoder {
  pub fn new(codec: u32, notify: Notify) -> io::Result<Self> {
    let (coding, split) = match codec.to_le_bytes() {
      [b'H', b'2', b'6', b'4'] => (7, 1u32),
      [b'H', b'E', b'V', b'C'] => (0x01000004, 1),
      [b'V', b'P', b'8', b'0'] => (9, 0),
      [b'V', b'P', b'9', b'0'] => (10, 0),
      [b'A', b'V', b'0', b'1'] => (0x01000008, 0),
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
    // SAFETY: mpp_create returns the published MppApi ABI. Check size
    // and required entries before calls.
    let valid = unsafe {
      let api = api.as_ref();
      api.size as usize >= std::mem::size_of::<Api>()
        && api.put.is_some()
        && api.get.is_some()
        && api.reset.is_some()
        && api.control.is_some()
    };
    if !valid {
      unsafe {
        mpp_destroy(context.as_ptr());
      }
      return Err(io::Error::other("incompatible MPP API table"));
    }
    let mut decoder = Self {
      context: Some(context),
      api,
      group: None,
      retired: Vec::new(),
      callback: Box::new(notify),
    };
    decoder
      .control(0x00310005, (&split as *const u32).cast_mut().cast())?;
    check("init", unsafe { mpp_init(context.as_ptr(), 0, coding) })?;
    let timeout = 0i64;
    decoder.control(
      0x00200007,
      (&timeout as *const i64).cast_mut().cast(),
    )?;
    decoder.control(
      0x00200006,
      (&timeout as *const i64).cast_mut().cast(),
    )?;
    let mut config = std::ptr::null_mut();
    check("config init", unsafe { mpp_dec_cfg_init(&mut config) })?;
    let config = Config(config);
    decoder.control(0x00310202, config.0)?;
    let callback = notify_ready as *const () as Handle;
    let context =
      (&*decoder.callback as *const Notify).cast_mut().cast();
    for (key, value) in [
      (c"cb:frm_rdy_cb", callback),
      (c"cb:frm_rdy_ctx", context),
      (c"cb:pkt_rdy_cb", callback),
      (c"cb:pkt_rdy_ctx", context),
    ] {
      check("callback config", unsafe {
        mpp_dec_cfg_set_ptr(config.0, key.as_ptr(), value)
      })?;
    }
    decoder.control(0x00310201, config.0)?;
    Ok(decoder)
  }

  fn control(
    &mut self,
    command: u32,
    parameter: Handle,
  ) -> io::Result<()> {
    unsafe {
      check(
        "control",
        self.api.as_ref().control.unwrap()(
          self.context.unwrap().as_ptr(),
          command,
          parameter,
        ),
      )
    }
  }

  pub fn packet(&mut self, packet: &Packet) -> io::Result<bool> {
    Ok(
      unsafe {
        self.api.as_ref().put.unwrap()(
          self.context.unwrap().as_ptr(),
          packet.handle(),
        )
      } == 0,
    )
  }

  pub fn frame(&mut self) -> io::Result<Option<Frame>> {
    self.retired.retain(Group::in_use);
    let mut raw = std::ptr::null_mut();
    let result = unsafe {
      self.api.as_ref().get.unwrap()(
        self.context.unwrap().as_ptr(),
        &mut raw,
      )
    };
    if result != 0 {
      return Ok(None);
    }
    Ok(unsafe { Frame::from_raw(raw) })
  }

  pub fn configure_output(&mut self, ten_bit: bool) -> io::Result<()> {
    let group = Group::new(ten_bit)?;
    self.control(0x00310002, group.handle())?;
    if let Some(previous) = self.group.replace(group) {
      self.retired.push(previous);
      self.retired.last_mut().unwrap().retire()?;
    }
    self.retired.retain(Group::in_use);
    Ok(())
  }

  pub fn has_output(&self) -> bool {
    self.group.is_some()
  }

  pub fn import_capture(
    &mut self,
    buffers: &[Buffer],
  ) -> io::Result<()> {
    self
      .group
      .as_mut()
      .ok_or_else(|| io::Error::other("decoder has no output group"))?
      .commit(buffers)
  }

  pub fn ready(&mut self) -> io::Result<()> {
    let group = self
      .group
      .as_ref()
      .ok_or_else(|| io::Error::other("decoder has no output group"))?
      .handle();
    self.control(0x00310002, group)?;
    self.control(0x00310003, std::ptr::null_mut())
  }

  pub fn reset(&mut self) -> io::Result<()> {
    unsafe {
      check(
        "reset",
        self.api.as_ref().reset.unwrap()(
          self.context.unwrap().as_ptr(),
        ),
      )
    }
  }

  pub fn close(mut self) -> io::Result<()> {
    self.dispose()
  }

  fn dispose(&mut self) -> io::Result<()> {
    let Some(context) = self.context.take() else {
      return Ok(());
    };
    let reset =
      unsafe { self.api.as_ref().reset.unwrap()(context.as_ptr()) };
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
