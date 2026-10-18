//! module: image; responsibility: describe borrowed or owned DMA-BUF
//! images. operations: plane layout access, image construction/borrow;
//! entities: Plane, Image. tests: image_tests and quantize_tests via
//! public image views.
use std::os::fd::{AsFd, BorrowedFd};

use crate::dma::Buffer;

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Depth {
  Eight = 8,
  Ten = 10,
}

#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
  fd: BorrowedFd<'a>,
  metadata:
    Option<&'a std::sync::OnceLock<crate::dma::system::Metadata>>,
  known: Option<crate::dma::system::Metadata>,
  stride: u32,
  rows: u32,
}
impl<'a> Plane<'a> {
  pub fn new(fd: BorrowedFd<'a>, stride: u32, rows: u32) -> Self {
    Self { fd, stride, rows, metadata: None, known: None }
  }

  pub(crate) fn from_buffer(
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

  pub(crate) fn registered(
    fd: BorrowedFd<'a>,
    stride: u32,
    rows: u32,
    metadata: &'a std::sync::OnceLock<crate::dma::system::Metadata>,
  ) -> Self {
    Self { fd, stride, rows, metadata: Some(metadata), known: None }
  }

  pub(crate) fn metadata(
    self,
  ) -> std::io::Result<crate::dma::system::Metadata> {
    if let Some(known) = self.known {
      return Ok(known);
    }
    if let Some(cache) = self.metadata {
      if let Some(value) = cache.get() {
        return Ok(*value);
      }
      let value = crate::dma::system::metadata(self.fd)?;
      return Ok(*cache.get_or_init(|| value));
    }
    crate::dma::system::metadata(self.fd)
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
