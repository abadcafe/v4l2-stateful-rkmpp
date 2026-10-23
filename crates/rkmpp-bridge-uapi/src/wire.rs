//! module: UAPI; responsibility: expose compiler-derived raw bridge
//! wire values. operations/entities: four POD records, three enums,
//! seven ABI constants; tests: lib_tests, independent service C layout
//! checks and Python native_tests.
//! operations: generated wire records and ABI constants;
include!(concat!(env!("OUT_DIR"), "/bridge.rs"));
