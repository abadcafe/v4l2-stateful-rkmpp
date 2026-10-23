//! module: rkmpp::ffi; responsibility: bind the selected public MPP
//! ABI. operations/entities: generated private declarations; tests:
//! public MPP object contracts and tests/support/mpp_sdk_tests.py
//! through Cargo.
include!(concat!(env!("OUT_DIR"), "/mpp.rs"));
