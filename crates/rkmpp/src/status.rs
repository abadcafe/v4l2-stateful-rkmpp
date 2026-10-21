//! module: rkmpp::status; responsibility: classify public nonblocking
//! MPP results. operations: frame, reservation; entities: none;
//! tests: status_tests.
use std::io;
fn failure(operation: &str, status: i32) -> io::Error {
  io::Error::other(format!("MPP {operation}: {status}"))
}
pub(super) fn frame(status: i32) -> io::Result<()> {
  if status == 0 { Ok(()) } else { Err(failure("get frame", status)) }
}
pub(super) fn reservation(status: i32) -> io::Result<bool> {
  match status {
    0 => Ok(true),
    -1 => Ok(false),
    _ => Err(failure("reserve external buffer", status)),
  }
}
