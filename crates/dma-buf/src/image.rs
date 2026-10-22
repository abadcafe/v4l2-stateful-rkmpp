//! module: image; responsibility: describe borrowed DMA-BUF images.
//! operations: plane layout access, metadata source retention;
//! entities: Depth, Plane, PlaneCache. tests:
//! image_tests and quantize_tests via public image views.
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};

use crate::memory::Buffer;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Depth {
  Eight = 8,
  Ten = 10,
}

#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
  fd: BorrowedFd<'a>,
  known: Option<crate::system::Metadata>,
  stride: u32,
  rows: u32,
}
impl<'a> Plane<'a> {
  pub fn new(fd: BorrowedFd<'a>, stride: u32, rows: u32) -> Self {
    Self { fd, stride, rows, known: None }
  }

  pub fn from_buffer(
    buffer: &'a Buffer,
    stride: u32,
    rows: u32,
  ) -> Self {
    Self {
      fd: buffer.as_fd(),
      stride,
      rows,
      known: Some(buffer.metadata()),
    }
  }

  pub fn metadata(self) -> std::io::Result<crate::system::Metadata> {
    if let Some(known) = self.known {
      return Ok(known);
    }
    crate::system::metadata(self.fd)
  }

  pub fn fd(self) -> BorrowedFd<'a> {
    self.fd
  }

  pub fn stride(self) -> u32 {
    self.stride
  }

  pub fn rows(self) -> u32 {
    self.rows
  }
}
/// Retain a DMA-BUF source and cache its first descriptor's metadata.
/// Sources selecting another live descriptor get fresh metadata for
/// that view; an AsFd implementation need not always select the same
/// fd.
#[derive(Debug)]
pub struct PlaneCache<T: AsFd> {
  source: T,
  metadata: std::sync::OnceLock<(RawFd, crate::Metadata)>,
}
impl<T: AsFd> PlaneCache<T> {
  pub fn new(source: T) -> Self {
    Self { source, metadata: std::sync::OnceLock::new() }
  }

  pub fn source(&self) -> &T {
    &self.source
  }

  pub fn plane(
    &self,
    stride: u32,
    rows: u32,
  ) -> std::io::Result<Plane<'_>> {
    let fd = self.source.as_fd();
    let known = match self.metadata.get() {
      Some((cached_fd, metadata)) if *cached_fd == fd.as_raw_fd() => {
        *metadata
      }
      _ => {
        let metadata = crate::system::metadata(fd)?;
        // A concurrent first access may select a different descriptor.
        // Return this lookup's metadata even if another fd wins the
        // cache.
        let _ = self.metadata.set((fd.as_raw_fd(), metadata));
        metadata
      }
    };
    Ok(Plane { fd, known: Some(known), stride, rows })
  }
}
