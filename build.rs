use std::env;
use std::path::PathBuf;
use std::process::Command;
fn main() {
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
