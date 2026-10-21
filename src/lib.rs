//! module: lib; responsibility: publish the bridge's owned module
//! contracts. operations: public module interfaces; entities: none;
//! tests: co-located module tests.
mod backend;
mod bitstream;
#[cfg(test)]
mod bitstream_tests;
mod diagnostics;
mod hardware;
mod notify;
mod pool;
mod protocol;
mod quantize;
mod service;
mod shutdown;
mod transport;

#[cfg(test)]
mod backend_tests;
#[cfg(test)]
mod diagnostics_tests;
#[cfg(test)]
mod hardware_tests;
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

mod timestamp;
#[cfg(test)]
mod timestamp_tests;

mod frontend;
#[cfg(test)]
mod frontend_tests;
mod picture;

mod submission;
#[cfg(test)]
mod submission_tests;

mod readiness;
#[cfg(test)]
mod readiness_tests;

mod fence;
#[cfg(test)]
mod fence_tests;

mod descriptor;
#[cfg(test)]
mod descriptor_tests;

/// Run the bridge service.
pub use service::serve;

mod execution;
#[cfg(test)]
mod execution_tests;

mod codec;

#[cfg(test)]
mod frontend_decoder_tests;
