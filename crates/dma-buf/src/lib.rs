//! module: dma_buf; responsibility: publish DMA storage contracts.
//! operations: memory, image, device, metadata and sync-file exports;
//! entities: none; tests: sibling module contract tests and service
//! integration.
mod access;
#[cfg(test)]
mod access_tests;
pub mod device;
#[cfg(test)]
mod device_tests;
pub mod image;
#[cfg(test)]
mod image_tests;
pub mod memory;
#[cfg(test)]
mod memory_tests;
mod storage;
#[cfg(test)]
mod storage_tests;
mod system;
#[cfg(test)]
mod system_tests;

pub use system::{Metadata, export_read, export_write};
