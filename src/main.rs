//! module: main; responsibility: execute the production CLI command.
//! operations: serve/help process commands; entities: none;
//! tests: tests/service/test_cli.
use std::env;
use std::path::Path;
use std::process::ExitCode;
fn main() -> ExitCode {
  let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
  let result = match args.as_slice() {
    [] => {
      println!(
        "v4l2-stateful-rkmpp: V4L2 / MPP bridge service\nCommands: \
         serve <control-device> <diagnostics-socket>"
      );
      Ok(())
    }
    [a] if a == "--help" => {
      println!(
        "v4l2-stateful-rkmpp: V4L2 / MPP bridge service\nCommands: \
         serve <control-device> <diagnostics-socket>"
      );
      Ok(())
    }
    [a, control, status] if a == "serve" => {
      match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
      {
        Ok(runtime) => {
          runtime.block_on(v4l2_stateful_rkmpp::service::serve(
            Path::new(control),
            Path::new(status),
          ))
        }
        Err(error) => Err(format!("create runtime: {error}")),
      }
    }
    _ => Err("unknown command; use --help".into()),
  };
  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}
