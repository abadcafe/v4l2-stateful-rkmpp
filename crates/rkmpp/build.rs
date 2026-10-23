//! module: rkmpp build; responsibility: derive the selected MPP SDK
//! ABI. operations: Cargo build entrypoint; entities: Symbols; tests:
//! tests/support/mpp_sdk_tests.py and rkmpp public contracts.
#[derive(Debug)]
struct Symbols;
impl bindgen::callbacks::ParseCallbacks for Symbols {
  fn enum_variant_behavior(
    &self,
    _name: Option<&str>,
    variant: &str,
    _value: bindgen::callbacks::EnumVariantValue,
  ) -> Option<bindgen::callbacks::EnumVariantCustomBehavior> {
    // Generate only the SDK operations this crate actually uses. There
    // is no warning suppression or public surface for unrelated
    // encoder controls.
    (!matches!(
      variant,
      "MPP_CTX_DEC"
        | "MPP_VIDEO_CodingAVC"
        | "MPP_VIDEO_CodingHEVC"
        | "MPP_VIDEO_CodingVP8"
        | "MPP_VIDEO_CodingVP9"
        | "MPP_VIDEO_CodingAV1"
        | "MPP_SET_INPUT_TIMEOUT"
        | "MPP_SET_OUTPUT_TIMEOUT"
        | "MPP_SET_DISABLE_THREAD"
        | "MPP_DEC_SET_PARSER_SPLIT_MODE"
        | "MPP_DEC_SET_ENABLE_DEINTERLACE"
        | "MPP_DEC_SET_IMMEDIATE_OUT"
        | "MPP_DEC_SET_EXT_BUF_GROUP"
        | "MPP_DEC_SET_INFO_CHANGE_READY"
        | "MPP_DEC_SET_CFG"
        | "MPP_DEC_GET_CFG"
        | "MPP_BUFFER_TYPE_DRM"
        | "MPP_BUFFER_INTERNAL"
        | "MPP_BUFFER_EXTERNAL"
    ))
    .then_some(bindgen::callbacks::EnumVariantCustomBehavior::Hide)
  }

  fn enum_variant_name(
    &self,
    _name: Option<&str>,
    variant: &str,
    _value: bindgen::callbacks::EnumVariantValue,
  ) -> Option<String> {
    Some(variant.replace("Coding", "CODING_").to_uppercase())
  }

  fn item_name(
    &self,
    item: bindgen::callbacks::ItemInfo<'_>,
  ) -> Option<String> {
    match item.name {
      "RK_U32" => Some("RkU32".into()),
      "RK_S32" => Some("RkS32".into()),
      "RK_U64" => Some("RkU64".into()),
      "RK_S64" => Some("RkS64".into()),
      "MPP_RET" => Some("MppRet".into()),
      _ => None,
    }
  }
}
fn main() {
  let flags = native_sdk::configure("rockchip_mpp");
  let output =
    std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
  bindgen::Builder::default()
    .formatter(bindgen::Formatter::Prettyplease)
    .header_contents(
      "mpp.h",
      "#include <rockchip/mpp_frame.h>\n#include <rockchip/rk_mpi.h>\n#include <rockchip/mpp_packet.h>\n#include <rockchip/rk_vdec_cfg.h>\n",
    )
    .clang_args(flags)
    .allowlist_function("mpp_(create|init|destroy|dec_cfg_(init|deinit|set_ptr)|buffer_(group_(get|put|clear|unused|usage|limit_config)|get_with_tag|import_with_tag|inc_ref_with_caller|put_with_caller|get_fd_with_caller|get_index_with_caller)|frame_(deinit|get_(width|height|hor_stride|ver_stride|buf_size|mode|dts|pts|errinfo|discard|eos|info_change|fmt|buffer|color_primaries|colorspace|color_trc|color_range))|packet_(init|deinit|set_(data|size|flag|pos|length|eos|pts|dts)|has_meta|get_(buffer|segment_nb|length)))")
    .allowlist_type("MppApi|MppBufferInfo")
    .parse_callbacks(Box::new(Symbols))
    .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
    .prepend_enum_name(false)
    .generate_comments(false)
    .layout_tests(false)
    .generate()
    .expect("generate selected MPP SDK bindings")
    .write_to_file(output.join("mpp.rs"))
    .expect("write MPP SDK bindings");
}
