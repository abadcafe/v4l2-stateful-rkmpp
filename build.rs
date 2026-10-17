//! module: build; responsibility: compile GPU code and installed public
//! FFI declarations. operations: cargo build entrypoint; entities:
//! CodecSymbols; tests: compiled bitstream and quantize
//! contracts.
use std::env;
use std::path::PathBuf;
use std::process::Command;
#[derive(Debug)]
struct CodecSymbols;
impl bindgen::callbacks::ParseCallbacks for CodecSymbols {
  fn item_name(
    &self,
    item: bindgen::callbacks::ItemInfo<'_>,
  ) -> Option<String> {
    match item.name {
      "gint8" => Some("GInt8".into()),
      "guint8" => Some("GUInt8".into()),
      "gint16" => Some("GInt16".into()),
      "guint16" => Some("GUInt16".into()),
      "gint64" => Some("GInt64".into()),
      "guint64" => Some("GUInt64".into()),
      "gint32" => Some("GInt32".into()),
      "guint32" => Some("GUInt32".into()),
      "gsize" => Some("GSize".into()),
      "gint" => Some("GInt".into()),
      "guint" => Some("GUInt".into()),
      "gboolean" => Some("GBoolean".into()),
      "GstAV1MetadataITUT_T35" => Some("GstAv1MetadataItutT35".into()),
      _ => None,
    }
  }

  fn field_name(
    &self,
    field: bindgen::callbacks::FieldInfo<'_>,
  ) -> Option<String> {
    field
      .field_name
      .chars()
      .any(char::is_uppercase)
      .then(|| field.field_name.to_lowercase())
  }

  fn enum_variant_behavior(
    &self,
    _name: Option<&str>,
    _variant: &str,
    _value: bindgen::callbacks::EnumVariantValue,
  ) -> Option<bindgen::callbacks::EnumVariantCustomBehavior> {
    Some(bindgen::callbacks::EnumVariantCustomBehavior::Hide)
  }
}
fn codec_parsers(output: &std::path::Path) {
  let flags = Command::new("pkg-config")
    .args(["--cflags", "gstreamer-codecparsers-1.0"])
    .output()
    .expect("query GStreamer codecparsers headers");
  assert!(
    flags.status.success(),
    "GStreamer codecparsers development files are required"
  );
  bindgen::Builder::default()
    .header_contents(
      "framed.h",
      "#include <gst/codecparsers/gstvp8parser.h>\n#include \
       <gst/codecparsers/gstvp9parser.h>\n#include \
       <gst/codecparsers/gstav1parser.h>\n#include <gst/codecparsers/gsth264parser.h>\n#include <gst/codecparsers/gsth265parser.h>",
    )
    .clang_args(
      String::from_utf8(flags.stdout).unwrap().split_whitespace(),
    )
    .clang_arg("-DGST_USE_UNSTABLE_API")
    .allowlist_function(
      "gst_h264_nal_parser_(new|free)|gst_h264_parser_(identify_nalu|parse_nal|parse_slice_hdr)|gst_h265_parser_(new|free|identify_nalu|parse_nal|parse_slice_hdr)|gst_h265_slice_hdr_free|gst_vp8_parser_.*|gst_vp9_parser_.*\
       |gst_av1_parser_(new|free|reset|identify_one_obu|parse_.*\
       _obu|reference_frame_update)",
    )
    .opaque_type(
      "GstAV1Parser|_GstAV1Parser|GstVp9Parser|_GstVp9Parser|GstH264NalParser|_GstH264NalParser|GstH265Parser|_GstH265Parser",
    )
    .parse_callbacks(Box::new(CodecSymbols))
    .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
    .layout_tests(false)
    .prepend_enum_name(false)
    .generate_comments(false)
    .generate()
    .expect("generate public codecparser bindings")
    .write_to_file(output.join("framed.rs"))
    .expect("write codecparser bindings");
  println!("cargo:rustc-link-lib=gstcodecparsers-1.0");
}
fn main() {
  let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
  codec_parsers(&output);
  let shader =
    PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("quantize.spv");
  let status = Command::new("glslangValidator")
    .args([
      "-V",
      "--target-env",
      "vulkan1.1",
      "shaders/quantize.comp",
      "-o",
    ])
    .arg(shader)
    .status()
    .expect("execute GLSL shader compiler");
  assert!(status.success(), "GPU shader compilation failed");
  println!("cargo:rerun-if-changed=shaders/quantize.comp");
}
