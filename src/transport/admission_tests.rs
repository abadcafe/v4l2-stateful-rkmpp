//! module: transport::admission_tests; responsibility: specify broker
//! fault scope. operations: tests; entities: none; tests: classify
//! errno by effect.
use std::io;

type Admission = super::admission::Admission<()>;
#[test]
fn test_classify_transient_local_pressure_and_fatal_errors() {
  for code in [libc::EINTR, libc::EAGAIN] {
    assert!(matches!(
      Admission::classify(io::Error::from_raw_os_error(code)).unwrap(),
      Admission::Idle
    ));
  }
  assert!(
    matches!(Admission::classify(io::Error::from_raw_os_error(libc::EPIPE)).unwrap(), Admission::Rejected(e) if e.raw_os_error() == Some(libc::EPIPE))
  );
  for code in [libc::ENOMEM, libc::EMFILE, libc::ENFILE] {
    assert!(
      matches!(Admission::classify(io::Error::from_raw_os_error(code)).unwrap(), Admission::Pressure(e) if e.raw_os_error() == Some(code))
    );
  }
  for code in
    [libc::EIO, libc::EBADF, libc::ENODEV, libc::ENOTTY, libc::EINVAL]
  {
    assert_eq!(
      Admission::classify(io::Error::from_raw_os_error(code))
        .err()
        .unwrap()
        .raw_os_error(),
      Some(code)
    );
  }
}
