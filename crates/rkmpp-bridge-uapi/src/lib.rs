//! module: bridge UAPI; responsibility: publish one Rust/Python bridge
//! protocol. operations: wire types and Python extension entrypoint;
//! entities: child contracts; tests: wire_tests and
//! tests/support/native_tests.py.
mod python;
pub mod wire;
#[cfg(test)]
mod wire_tests;
