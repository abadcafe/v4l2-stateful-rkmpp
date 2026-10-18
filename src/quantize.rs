//! module: quantize; responsibility: produce NV12 on the GPU from
//! decoded eight-bit or packed ten-bit images. Eight-bit samples are
//! preserved; ten-bit samples are dithered. operations: new, submit,
//! completion, finish; entities: Quantizer; tests: quantize_tests. No
//! CPU pixel maps, no per-frame fd duplication after an import is
//! cached.
use std::io;
use std::num::NonZeroUsize;

use crate::dma::system as dma;
use crate::image::{Depth, Plane};
mod context;
#[cfg(test)]
mod context_tests;
mod device;
#[cfg(test)]
mod device_tests;
mod imports;
#[cfg(test)]
mod imports_tests;
mod layout;
#[cfg(test)]
mod layout_tests;
use context::Context;
use imports::Cache;
use layout::Layout;

struct Prepared {
  buffers: imports::Pair,
  layout: Layout,
  dependencies: [crate::fence::Fence; 2],
  next: usize,
}
pub struct Quantizer {
  context: Context,
  imports: Cache,
  failed: bool,
  prepared: Option<Prepared>,
}
impl Quantizer {
  pub fn new() -> io::Result<Self> {
    let context = Context::new()?;
    let imports =
      Cache::new(context.device(), NonZeroUsize::new(64).unwrap())?;
    Ok(Self { context, imports, failed: false, prepared: None })
  }

  pub fn submit(
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
    if !self.finish()? {
      return Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "conversion still owns its command resources",
      ));
    }
    let layout =
      Layout::new(depth, width, height, source, destination)?;
    let src = source.metadata()?;
    let dst = destination.metadata()?;
    if (src.st_dev == dst.st_dev && src.st_ino == dst.st_ino)
      || src.st_size < layout.source_size() as i64
      || dst.st_size < layout.destination_size() as i64
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "undersized or aliased conversion buffers",
      ));
    }
    let source_ready =
      crate::fence::Fence::new(Some(dma::export_read(source.fd())?));
    let target_ready = crate::fence::Fence::new(Some(
      dma::export_write(destination.fd())?,
    ));
    let buffers = self.imports.pair(source, destination)?;
    self.prepared = Some(Prepared {
      buffers,
      layout,
      dependencies: [source_ready, target_ready],
      next: 0,
    });
    Ok(())
  }

  pub fn completion(&self) -> Option<crate::descriptor::Lease> {
    self
      .prepared
      .as_ref()
      .and_then(|p| p.dependencies.get(p.next))
      .and_then(crate::fence::Fence::lease)
      .or_else(|| self.context.completion())
  }

  pub fn finish(&mut self) -> io::Result<bool> {
    if self.failed {
      return Err(io::Error::other(
        "Vulkan quantizer failed previously",
      ));
    }
    if let Some(prepared) = self.prepared.as_mut() {
      while let Some(fence) = prepared.dependencies.get(prepared.next) {
        if !fence.ready()? {
          return Ok(false);
        }
        prepared.next += 1;
      }
      let prepared = self.prepared.take().unwrap();
      match self.context.dispatch(prepared.buffers, prepared.layout) {
        Ok(()) => {}
        Err(error) => {
          self.failed = true;
          let _ = self.context.quiesce();
          return Err(error);
        }
      }
    }
    self.context.finish()
  }
}
