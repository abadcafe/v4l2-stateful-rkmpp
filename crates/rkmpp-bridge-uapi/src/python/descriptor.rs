//! module: descriptor; responsibility: own a Python Descriptor wire
//! value. operations: typed fields, encoding and bounded record access;
//! entities: Descriptor; tests: native_tests.
use super::record::record;
use crate::wire as raw;
record!(Descriptor, raw::Descriptor; arrays [];
  fields {
    fd => set_fd: i32 = [fd],
    size => set_size: u32 = [size],
  }
  methods {

  }
);
