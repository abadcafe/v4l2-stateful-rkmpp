//! module: UAPI; responsibility: expose compiler-derived raw bridge
//! wire values. operations/entities: four POD records, three enums,
//! seven ABI constants; tests: lib_tests, independent service C layout
//! checks and Python native_tests.
include!(concat!(env!("OUT_DIR"), "/bridge.rs"));

impl Message {
  pub const CAPTURE_REUSE: u32 = Flags::CaptureReuse as u32;
  pub const FRAME_ERROR: u32 = Flags::FrameError as u32;
  pub const RESET_ABORT_DRAIN: u32 = Flags::ResetAbortDrain as u32;
  pub const RESET_CAPTURE_ONLY: u32 = Flags::ResetCaptureOnly as u32;
}
impl Batch {
  pub const IOCTL: std::ffi::c_ulong = RB_IOCTL_EXCHANGE;
}
impl Pool {
  pub const IOCTL: std::ffi::c_ulong = RB_IOCTL_REGISTER_POOL;
}
