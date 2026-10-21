//! module: UAPI build; responsibility: generate the shared raw ABI
//! once. operations: Cargo build entrypoint; entities: BridgeSymbols;
//! tests: UAPI contract tests, protocol C checks and native_tests.
#[derive(Debug)]
struct BridgeSymbols;
impl bindgen::callbacks::ParseCallbacks for BridgeSymbols {
  fn add_derives(
    &self,
    info: &bindgen::callbacks::DeriveInfo<'_>,
  ) -> Vec<String> {
    if matches!(info.name, "Message" | "Batch" | "Descriptor" | "Pool")
    {
      vec!["bytemuck::Pod".into(), "bytemuck::Zeroable".into()]
    } else {
      Vec::new()
    }
  }

  fn blocklisted_type_implements_trait(
    &self,
    name: &str,
    _: bindgen::callbacks::DeriveTrait,
  ) -> Option<bindgen::callbacks::ImplementsTrait> {
    matches!(name, "__u32" | "__u64" | "__s32" | "U32" | "U64" | "I32")
      .then_some(bindgen::callbacks::ImplementsTrait::Yes)
  }

  fn item_name(
    &self,
    item: bindgen::callbacks::ItemInfo<'_>,
  ) -> Option<String> {
    Some(
      match item.name {
        "__u32" => "U32",
        "__s32" => "I32",
        "__u64" => "U64",
        "rb_message" => "Message",
        "rb_batch" => "Batch",
        "rb_pool" => "Pool",
        "rb_registered_buffer" => "Descriptor",
        "rb_event" => "Event",
        "rb_reply" => "Reply",
        "rb_message_flags" => "Flags",
        _ => return None,
      }
      .into(),
    )
  }

  fn field_name(
    &self,
    field: bindgen::callbacks::FieldInfo<'_>,
  ) -> Option<String> {
    (field.field_name == "type").then(|| "kind".into())
  }

  fn enum_variant_name(
    &self,
    _: Option<&str>,
    name: &str,
    _: bindgen::callbacks::EnumVariantValue,
  ) -> Option<String> {
    name.strip_prefix("RB_").map(|name| {
      name
        .strip_suffix("_FLAG")
        .unwrap_or(name)
        .split('_')
        .map(|part| {
          let mut chars = part.chars();
          chars.next().unwrap().to_uppercase().to_string()
            + &chars.as_str().to_lowercase()
        })
        .collect()
    })
  }
}
fn builder(include: &std::path::Path) -> bindgen::Builder {
  bindgen::Builder::default()
    .formatter(bindgen::Formatter::Prettyplease)
    .header_contents("bridge.h", r#"#include "bridge_uapi.h"
static const unsigned long RB_IOCTL_ACCEPT = RB_ACCEPT;
static const unsigned long RB_IOCTL_EXCHANGE = RB_EXCHANGE;
static const unsigned long RB_IOCTL_REGISTER_POOL = RB_REGISTER_POOL;
enum rb_message_flags { RB_CAPTURE_REUSE_FLAG = RB_CAPTURE_REUSE, RB_FRAME_ERROR_FLAG = RB_FRAME_ERROR, RB_RESET_CAPTURE_ONLY_FLAG = RB_RESET_CAPTURE_ONLY, RB_RESET_ABORT_DRAIN_FLAG = RB_RESET_ABORT_DRAIN };
"#)
    .clang_arg(format!("-I{}",include.display()))
    .allowlist_type("rb_.*")
    .blocklist_type("__.*")
    .raw_line("use std::primitive::{u32 as U32, u64 as U64, i32 as I32};")
    .allowlist_var("RB_VERSION|RB_BATCH_MAX|RB_MAX_BUFFERS|RB_MIN_CAPTURE|RB_IOCTL_.*")
    .rustified_enum("rb_event|rb_reply|rb_message_flags")
    .parse_callbacks(Box::new(BridgeSymbols))
    .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
}

fn main() {
  let root =
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let output =
    std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
  builder(&root.join("kernel"))
    .derive_default(true)
    .derive_partialeq(true)
    .derive_eq(true)
    .layout_tests(false)
    .generate_comments(false)
    .generate()
    .expect("generate shared bridge ABI")
    .write_to_file(output.join("bridge.rs"))
    .expect("write shared bridge ABI");
}
