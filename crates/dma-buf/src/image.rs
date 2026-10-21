//! module: image; responsibility: describe borrowed or owned DMA-BUF
//! images. operations: plane layout access, image construction/borrow;
//! entities: Plane, Image. tests: image_tests and quantize_tests via
//! public image views.
use std::os::fd::{AsFd, BorrowedFd};

use crate::Buffer;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Depth {
  Eight = 8,
  Ten = 10,
}

#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
  fd: BorrowedFd<'a>,
  metadata: Option<&'a PlaneCache>,
  known: Option<crate::system::Metadata>,
  stride: u32,
  rows: u32,
}
impl<'a> Plane<'a> {
  pub fn new(fd: BorrowedFd<'a>, stride: u32, rows: u32) -> Self {
    Self { fd, stride, rows, metadata: None, known: None }
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
      metadata: None,
      known: Some(buffer.metadata()),
    }
  }

  pub fn registered(
    fd: BorrowedFd<'a>,
    stride: u32,
    rows: u32,
    metadata: &'a PlaneCache,
  ) -> Self {
    Self { fd, stride, rows, metadata: Some(metadata), known: None }
  }

  pub fn metadata(self) -> std::io::Result<crate::system::Metadata> {
    if let Some(known) = self.known {
      return Ok(known);
    }
    if let Some(cache) = self.metadata {
      if let Some(value) = cache.0.get() {
        return Ok(*value);
      }
      let value = crate::system::metadata(self.fd)?;
      return Ok(*cache.0.get_or_init(|| value));
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
#[derive(Clone, Debug)]
pub struct Image {
  buffer: Buffer,
  stride: u32,
  rows: u32,
}
impl Image {
  pub fn new(buffer: Buffer, stride: u32, rows: u32) -> Self {
    Self { buffer, stride, rows }
  }

  pub fn plane(&self) -> Plane<'_> {
    Plane::from_buffer(&self.buffer, self.stride, self.rows)
  }
}

/// Metadata retained by the owner of a borrowed DMA image.
#[derive(Default, Debug)]
pub struct PlaneCache(std::sync::OnceLock<crate::Metadata>);
