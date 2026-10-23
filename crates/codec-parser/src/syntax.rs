//! module: syntax; responsibility: normalize immutable coded sequence
//! metadata. operations: codec parsers, bounded RBSP reader, parameter
//! registry; entities: child contracts; tests: sibling child tests and
//! admission snapshots.
pub mod bits;
#[cfg(test)]
mod bits_tests;
pub mod geometry;
pub mod h264;
#[cfg(test)]
mod h264_tests;
pub mod hevc;
#[cfg(test)]
mod hevc_tests;
mod restriction;
#[cfg(test)]
mod restriction_tests;
