//! module: message; responsibility: own a Python Message wire value.
//! operations: typed fields, encoding and bounded record access;
//! entities: Message; tests: native_tests.
use super::record::record;
use crate::wire as raw;
record!(Message, raw::Message; arrays [];
  fields {
    version => set_version: u32 = [version],
    kind => set_kind: u32 = [kind],
    epoch => set_epoch: u64 = [epoch],
    index => set_index: u32 = [index],
    flags => set_flags: u32 = [flags],
    timestamp => set_timestamp: u64 = [timestamp],
    bytesused => set_bytesused: u32 = [bytesused],
    codec => set_codec: u32 = [codec],
    width => set_width: u32 = [width],
    height => set_height: u32 = [height],
    stride => set_stride: u32 = [stride],
    rows => set_rows: u32 = [rows],
    size => set_size: u32 = [size],
    format => set_format: u32 = [format],
    count => set_count: u32 = [count],
    generation => set_generation: u32 = [generation],
    colorspace => set_colorspace: u32 = [colorspace],
    xfer_func => set_xfer_func: u32 = [xfer_func],
    ycbcr_enc => set_ycbcr_enc: u32 = [ycbcr_enc],
    quantization => set_quantization: u32 = [quantization],
    pool_id => set_pool_id: u32 = [pool_id],
    data_offset => set_data_offset: u32 = [data_offset],
    crop_left => set_crop_left: u32 = [crop_left],
    crop_top => set_crop_top: u32 = [crop_top],
    coded_width => set_coded_width: u32 = [coded_width],
    coded_height => set_coded_height: u32 = [coded_height],
    field => set_field: u32 = [field],
    reserved => set_reserved: u32 = [reserved[0]],
  }
  methods {
    #[classattr]
    const CAPTURE_REUSE: u32 = raw::Flags::CaptureReuse as u32;
    #[classattr]
    const FRAME_ERROR: u32 = raw::Flags::FrameError as u32;
    #[classattr]
    const RESET_ABORT_DRAIN: u32 = raw::Flags::ResetAbortDrain as u32;
    #[classattr]
    const RESET_CAPTURE_ONLY: u32 = raw::Flags::ResetCaptureOnly as u32;
  }
);
