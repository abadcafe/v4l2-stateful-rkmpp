//! module: native_sdk; responsibility: discover one native SDK for both
//! compilation and linkage. operations: configure; entities: none;
//! tests: tests/support/build_configuration_tests.py and
//! mpp_sdk_tests.py through Cargo's public build protocol.
use std::ffi::OsString;
use std::process::Command;

// Use pkg-config's Rust discovery precedence for every raw query too.
// Otherwise headers/metadata can come from a different SDK than
// libraries.
fn selection_keys(variable: &str) -> [String; 4] {
  let target = std::env::var("TARGET").expect("Cargo target");
  let host = std::env::var("HOST").expect("Cargo host");
  let kind = if target == host { "HOST" } else { "TARGET" };
  [
    format!("{variable}_{target}"),
    format!("{variable}_{}", target.replace('-', "_")),
    format!("{kind}_{variable}"),
    variable.to_owned(),
  ]
}

fn selected(variable: &str) -> Option<OsString> {
  selection_keys(variable).iter().find_map(std::env::var_os)
}

fn query(arguments: &[&str]) -> String {
  let program =
    selected("PKG_CONFIG").unwrap_or_else(|| "pkg-config".into());
  let mut command = Command::new(program);
  for variable in
    ["PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR"]
  {
    if let Some(value) = selected(variable) {
      command.env(variable, value);
    }
  }
  let output =
    command.args(arguments).output().expect("execute pkg-config");
  assert!(
    output.status.success(),
    "pkg-config {arguments:?}: {}",
    String::from_utf8_lossy(&output.stderr),
  );
  String::from_utf8(output.stdout)
    .expect("pkg-config output must be UTF-8")
}

fn metadata_inputs(package: &str) {
  let mut pending = vec![package.to_owned()];
  let mut seen = std::collections::HashSet::new();
  let mut directories = std::collections::BTreeSet::new();
  while let Some(package) = pending.pop() {
    if !seen.insert(package.clone()) {
      continue;
    }
    let directory = query(&["--variable=pcfiledir", &package]);
    let directory = directory.trim_end_matches('\n');
    assert!(!directory.is_empty(), "SDK metadata directory is missing");
    // This public query exposes the provider's directory, not its
    // filename. Include uninstalled/virtual providers without guessing.
    directories.insert(std::path::PathBuf::from(directory));
    let dependencies = query(&[
      "--print-requires",
      "--print-requires-private",
      &package,
    ]);
    pending.extend(dependencies.lines().filter_map(|line| {
      line.split_whitespace().next().map(str::to_owned)
    }));
  }
  for variable in ["PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR"] {
    if let Some(paths) = selected(variable) {
      directories.extend(
        std::env::split_paths(&paths)
          .filter(|path| !path.as_os_str().is_empty()),
      );
    }
  }
  for directory in directories {
    println!(
      "cargo:rerun-if-changed={}",
      directory.join(".").display()
    );
  }
  if let Some(files) = std::env::var_os("PKG_CONFIG_PRELOADED_FILES") {
    for file in std::env::split_paths(&files) {
      println!("cargo:rerun-if-changed={}", file.display());
    }
  }
}

pub fn configure(package: &str) -> Vec<String> {
  // Cargo cannot observe environment reads in the pkg-config child.
  for variable in [
    "PKG_CONFIG",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
  ] {
    for key in selection_keys(variable) {
      println!("cargo:rerun-if-env-changed={key}");
    }
  }
  for variable in [
    "PKG_CONFIG_TOP_BUILD_DIR",
    "PKG_CONFIG_DISABLE_UNINSTALLED",
    "PKG_CONFIG_ALLOW_SYSTEM_CFLAGS",
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS",
    "PKG_CONFIG_SYSTEM_INCLUDE_PATH",
    "PKG_CONFIG_SYSTEM_LIBRARY_PATH",
    "PKG_CONFIG_DONT_DEFINE_PREFIX",
    "PKG_CONFIG_DONT_RELOCATE_PATHS",
    "PKG_CONFIG_FDO_SYSROOT_RULES",
    "PKG_CONFIG_IGNORE_CONFLICTS",
    "PKG_CONFIG_MAXIMUM_TRAVERSE_DEPTH",
    "PKG_CONFIG_MSVC_SYNTAX",
    "PKG_CONFIG_PRELOADED_FILES",
    "PKG_CONFIG_PURE_DEPGRAPH",
    "PKG_CONFIG_RELOCATE_PATHS",
    "CPATH",
    "C_INCLUDE_PATH",
    "CPLUS_INCLUDE_PATH",
    "OBJC_INCLUDE_PATH",
    "DESTDIR",
  ] {
    println!("cargo:rerun-if-env-changed={variable}");
  }
  metadata_inputs(package);
  pkg_config::Config::new()
    .statik(false)
    .probe_cflags(false)
    .print_system_libs(false)
    .print_system_cflags(false)
    .probe(package)
    .expect("discover native SDK libraries");
  // Keep complete compiler arguments: Library's parsed fields omit
  // arbitrary flags and cannot represent every preprocessor definition.
  shlex::split(&query(&["--cflags", package]))
    .expect("pkg-config compiler flags must have valid shell quoting")
}
