//! module: quantize; responsibility: produce NV12 on the GPU from
//! decoded eight-bit or packed ten-bit images. Eight-bit samples are
//! preserved; ten-bit samples are dithered. operations: new, convert;
//! entities: Quantizer; tests: quantize_tests. No CPU pixel maps, no
//! per-frame fd duplication after an import is cached.
use std::io;
use std::num::NonZeroUsize;

use crate::dma::system as dma;
use crate::image::{Depth, Plane};
mod context;
#[cfg(test)]
mod context_tests;
mod imports;
#[cfg(test)]
mod imports_tests;
mod layout;
#[cfg(test)]
mod layout_tests;
use context::Context;
use imports::Cache;
use layout::Layout;

pub struct Quantizer {
  context: Context,
  imports: Cache,
  failed: bool,
}
impl Quantizer {
  pub fn new() -> io::Result<Self> {
    let context = Context::new()?;
    let imports =
      Cache::new(context.device(), NonZeroUsize::new(64).unwrap())?;
    Ok(Self { context, imports, failed: false })
  }

  pub fn convert(
    &mut self,
    depth: Depth,
    width: u32,
    height: u32,
    source: Plane<'_>,
    destination: Plane<'_>,
  ) -> io::Result<()> {
    if self.failed {
      return Err(io::Error::other(
        "Vulkan quantizer failed previously",
      ));
    }
    let layout =
      Layout::new(depth, width, height, source, destination)?;
    let src = dma::metadata(source.fd())?;
    let dst = dma::metadata(destination.fd())?;
    if (src.st_dev == dst.st_dev && src.st_ino == dst.st_ino)
      || src.st_size < layout.source_size() as i64
      || dst.st_size < layout.destination_size() as i64
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "undersized or aliased conversion buffers",
      ));
    }
    dma::wait_read(source.fd())?;
    dma::wait_write(destination.fd())?;
    let buffers =
      self.imports.pair(source.fd(), &src, destination.fd(), &dst)?;
    let result = self.context.dispatch(buffers, layout);
    if result.is_err() {
      self.failed = true;
      // Preserve the input/output leases through any outstanding GPU
      // work before publishing an error.
      let _ = self.context.quiesce();
    }
    result
  }
}
