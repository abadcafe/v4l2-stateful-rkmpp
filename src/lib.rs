//! module: lib; responsibility: publish the bridge's owned module
//! contracts. operations: public module interfaces; entities: none;
//! tests: co-located module tests.
pub mod backend;
pub mod bitstream;
#[cfg(test)]
mod bitstream_tests;
pub mod converter;
pub mod diagnostics;
pub mod dma;
pub mod image;
pub mod mpp;
pub mod notify;
pub mod pool;
pub mod protocol;
pub mod quantize;
pub mod service;
pub mod shutdown;
pub mod transport;

#[cfg(test)]
mod backend_tests;
#[cfg(test)]
mod converter_tests;
#[cfg(test)]
mod diagnostics_tests;
#[cfg(test)]
mod dma_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod mpp_tests;
#[cfg(test)]
mod notify_tests;
#[cfg(test)]
mod pool_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod quantize_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod shutdown_tests;
#[cfg(test)]
mod transport_tests;

mod packet_syntax;
#[cfg(test)]
mod packet_syntax_tests;

pub mod timestamp;
#[cfg(test)]
mod timestamp_tests;

mod frontend;
#[cfg(test)]
mod frontend_tests;
pub mod picture;

mod submission;

mod task;
#[cfg(test)]
mod task_tests;
