"""module: build_configuration_tests; responsibility: validate SDK build inputs.

operations: pytest cases; entities: CargoMessage; tests: SDK selection and linking.
Only Cargo's public artifact/build-script protocol and pkg-config are used.
"""

import os
import shlex
import subprocess
from pathlib import Path
from typing import Literal
from uuid import uuid4

import pytest
from pydantic import BaseModel

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = "gstreamer-codecparsers-1.0"
type ConfigurationChange = Literal["environment", "metadata", "dependency"]


class CargoMessage(BaseModel):
    reason: str
    manifest_path: Path | None = None
    filenames: tuple[Path, ...] = ()


class WorkspaceMetadata(BaseModel):
    workspace_members: tuple[str, ...]
    workspace_default_members: tuple[str, ...]


def _run(
    arguments: list[str],
    directory: Path,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    process = subprocess.run(
        arguments,
        cwd=directory,
        env=environment,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert process.returncode == 0, process.stdout + process.stderr
    return process


def test_workspace_default_commands_cover_every_member() -> None:
    metadata = WorkspaceMetadata.model_validate_json(
        _run(["cargo", "metadata", "--no-deps", "--format-version=1"], ROOT).stdout
    )
    assert set(metadata.workspace_default_members) == set(metadata.workspace_members)


def _metadata(
    directory: Path,
    flags: str,
    libraries: str,
    version: str,
    requires: str = "",
) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    _ = (directory / f"{PACKAGE}.pc").write_text(
        "\n".join(
            [
                "Name: codec SDK build input probe",
                "Description: Installed SDK with controlled build configuration",
                f"Version: {version}",
                f"Cflags: {flags}",
                f"Libs: {libraries}",
                f"Requires: {requires}",
                "",
            ]
        )
    )


def test_generate_bindings_accepts_pkg_config_include_path_with_spaces(
    tmp_path: Path,
) -> None:
    crate = ROOT / "crates/codec-parser"
    compiled = _run(
        [
            "cargo",
            "check",
            "--locked",
            "--offline",
            "-p",
            "codec-parser",
            "--message-format=json",
        ],
        ROOT,
    )
    messages = [
        CargoMessage.model_validate_json(line) for line in compiled.stdout.splitlines()
    ]
    scripts = [
        file
        for message in messages
        if message.reason == "compiler-artifact"
        and message.manifest_path == crate / "Cargo.toml"
        for file in message.filenames
        if file.name == "build-script-build"
    ]
    assert len(scripts) == 1
    flags = _run(["pkg-config", "--cflags", PACKAGE], ROOT).stdout.strip()
    libraries = _run(["pkg-config", "--libs", PACKAGE], ROOT).stdout.strip()
    version = _run(["pkg-config", "--modversion", PACKAGE], ROOT).stdout.strip()
    include = next(
        Path(flag[2:])
        for flag in shlex.split(flags)
        if flag.startswith("-I")
        and (Path(flag[2:]) / "gst/codecparsers/gstvp8parser.h").is_file()
    )
    sdk = tmp_path / "SDK with spaces"
    sdk.mkdir()
    alias = sdk / "include"
    alias.symlink_to(include, target_is_directory=True)
    metadata = tmp_path / "pkgconfig"
    _metadata(metadata, f"-I{shlex.quote(str(alias))} {flags}", libraries, version)
    environment = dict(os.environ)
    environment["PKG_CONFIG_PATH"] = str(metadata)
    quoted = _run(["pkg-config", "--cflags", PACKAGE], ROOT, environment)
    assert f"-I{alias}" in shlex.split(quoted.stdout)
    details = _run(["rustc", "-vV"], ROOT).stdout
    host = next(
        line.removeprefix("host: ")
        for line in details.splitlines()
        if line.startswith("host: ")
    )
    output = tmp_path / "bindings with spaces"
    output.mkdir()
    environment.update(
        OUT_DIR=str(output),
        HOST=host,
        TARGET=host,
        CARGO_MANIFEST_DIR=str(crate),
        CARGO_CFG_TARGET_OS="linux",
    )
    generated = _run([str(scripts[0])], crate, environment)
    for variable in ("PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR"):
        assert f"cargo:rerun-if-env-changed={variable}" in generated.stdout
    assert (output / "framed.rs").stat().st_size > 0


@pytest.mark.parametrize("change", ["environment", "metadata", "dependency"])
def test_generate_bindings_rechecks_changed_sdk_selection(
    tmp_path: Path, change: ConfigurationChange
) -> None:
    flags = _run(["pkg-config", "--cflags", PACKAGE], ROOT).stdout.strip()
    libraries = _run(["pkg-config", "--libs", PACKAGE], ROOT).stdout.strip()
    version = _run(["pkg-config", "--modversion", PACKAGE], ROOT).stdout.strip()
    include = tmp_path / "include"
    header = include / "gst/codecparsers/gstvp8parser.h"
    header.parent.mkdir(parents=True)
    # This fixture adds no SDK implementation. Its conditional error
    # distinguishes selecting new flags from using stale build output.
    _ = header.write_text(
        "\n".join(
            [
                "#ifdef SDK_BUILD_INPUT_REJECT",
                "#error SDK_CONFIGURATION_CHANGED",
                "#endif",
                "#include_next <gst/codecparsers/gstvp8parser.h>",
                "",
            ]
        )
    )
    accepted, rejected = tmp_path / "accepted", tmp_path / "rejected"
    selected = f"-I{shlex.quote(str(include))} {flags}"
    requirement = "sdk-build-input"
    rejected_flags = selected + " -DSDK_BUILD_INPUT_REJECT=1"
    _metadata(accepted, selected, libraries, version, requirement)
    _metadata(rejected, rejected_flags, libraries, version, requirement)
    dependency_source = "\n".join(
        [
            "Name: SDK build input dependency",
            "Description: Controlled transitive SDK flags",
            "Version: 1",
            "Cflags:",
            "",
        ]
    )
    dependency = accepted / f"{requirement}.pc"
    _ = dependency.write_text(dependency_source)
    _ = (rejected / dependency.name).write_text(dependency_source)
    environment = dict(os.environ)
    # Bindgen already tracks this input: force the initial baseline
    # independently of the SDK selection invalidation under test.
    environment["BINDGEN_EXTRA_CLANG_ARGS"] = f"-DSDK_BUILD_RUN_{uuid4().hex}=1"
    environment["PKG_CONFIG_PATH"] = str(accepted)
    command = ["cargo", "check", "--locked", "--offline", "-p", "codec-parser"]
    _ = _run(command, ROOT, environment)
    if change == "environment":
        environment["PKG_CONFIG_PATH"] = str(rejected)
    elif change == "metadata":
        _metadata(accepted, rejected_flags, libraries, version, requirement)
    else:
        _ = dependency.write_text(
            dependency_source.replace("Cflags:", "Cflags: -DSDK_BUILD_INPUT_REJECT=1")
        )
    changed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert changed.returncode != 0, "Cargo reused bindings from the previous SDK"
    assert "SDK_CONFIGURATION_CHANGED" in changed.stderr


@pytest.mark.parametrize("variable", ["PKG_CONFIG_PATH", "HOST_PKG_CONFIG_PATH"])
def test_link_uses_pkg_config_library_provider(tmp_path: Path, variable: str) -> None:
    flags = _run(["pkg-config", "--cflags", PACKAGE], ROOT).stdout.strip()
    libraries = _run(["pkg-config", "--libs", PACKAGE], ROOT).stdout.strip()
    version = _run(["pkg-config", "--modversion", PACKAGE], ROOT).stdout.strip()
    sdk = tmp_path / "SDK library with spaces"
    sdk.mkdir()
    admission = sdk / "selection.h"
    _ = admission.write_text(
        "\n".join(
            [
                "#ifndef SDK_LINK_CONFIG",
                "#error SDK headers and libraries selected different configurations",
                "#endif",
                "",
            ]
        )
    )
    source = sdk / "provider.rs"
    # An isolated Rust test library exposes a deterministic allocation failure.
    # This is a link-selection probe, not a production SDK wrapper.
    _ = source.write_text(
        "\n".join(
            [
                line
                for symbol in (
                    "gst_h264_nal_parser_new",
                    "gst_h265_parser_new",
                    "gst_av1_parser_new",
                    "gst_vp9_parser_new",
                )
                for line in (
                    "#[unsafe(no_mangle)]",
                    f'pub extern "C" fn {symbol}() -> *mut std::ffi::c_void {{',
                    "    std::ptr::null_mut()",
                    "}",
                    "",
                )
            ]
        )
    )
    _ = _run(
        [
            "rustc",
            "--edition=2024",
            "--crate-type=cdylib",
            "-Dwarnings",
            str(source),
            "-o",
            str(sdk / "libcodec_sdk_probe.so"),
        ],
        ROOT,
    )
    metadata = tmp_path / "pkgconfig"
    _metadata(
        metadata,
        f"-DSDK_LINK_CONFIG=1 {flags}",
        f"-L{shlex.quote(str(sdk))} -lcodec_sdk_probe {libraries}",
        version,
    )
    consumer = tmp_path / "consumer"
    consumer.mkdir()
    _ = (consumer / "Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                'name = "sdk-link-consumer"',
                'version = "0.0.0"',
                'edition = "2024"',
                "[dependencies]",
                f'codec-parser = {{ path = "{ROOT / "crates/codec-parser"}" }}',
                "[profile.dev.build-override]",
                "debug = true",
                "",
            ]
        )
    )
    main = consumer / "src/main.rs"
    main.parent.mkdir()
    _ = main.write_text(
        "\n".join(
            [
                "fn main() {",
                "    let mut errors = Vec::new();",
                '    for codec in [*b"H264", *b"HEVC", *b"AV01", *b"VP90"] {',
                "        let parser = codec_parser::Validator::new(u32::from_le_bytes(codec));",
                '        assert!(parser.is_err(), "SDK library provider was ignored: {codec:?}");',
                "        errors.push((codec, parser.err().unwrap().kind()));",
                "    }",
                "    assert!(errors.iter().all(|(_, kind)| *kind == std::io::ErrorKind::OutOfMemory),",
                '        "constructor failures must remain resource errors: {errors:?}");',
                "}",
                "",
            ]
        )
    )
    environment = dict(os.environ)
    environment[variable] = str(metadata)
    environment["BINDGEN_EXTRA_CLANG_ARGS"] = shlex.join(["-include", str(admission)])
    _ = _run(
        [
            "cargo",
            "build",
            "--offline",
            "--manifest-path",
            str(consumer / "Cargo.toml"),
            "--target-dir",
            str(ROOT / "target"),
        ],
        ROOT,
        environment,
    )
    # Loading uses the selected search directory without overriding symbols.
    environment["LD_LIBRARY_PATH"] = os.pathsep.join(
        [str(sdk), environment.get("LD_LIBRARY_PATH", "")]
    )
    _ = _run([str(ROOT / "target/debug/sdk-link-consumer")], ROOT, environment)
