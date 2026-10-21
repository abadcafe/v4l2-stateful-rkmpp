//! module: image; responsibility: describe borrowed or owned DMA-BUF
//! images. operations: plane layout access, image/cache construction
//! and borrow; entities: Depth, Plane, Image, PlaneCache. tests:
//! image_tests and quantize_tests via public image views.
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

/// An owned descriptor source and its inseparable cached metadata.
/// No constructor accepts independently supplied metadata or a second
/// fd.
#[derive(Debug)]
pub struct PlaneCache<T: AsFd> {
  source: T,
  metadata: std::sync::OnceLock<crate::Metadata>,
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
    let known = if let Some(metadata) = self.metadata.get() {
      *metadata
    } else {
      let metadata = crate::system::metadata(fd)?;
      *self.metadata.get_or_init(|| metadata)
    };
    Ok(Plane { fd, known: Some(known), stride, rows })
  }
}
