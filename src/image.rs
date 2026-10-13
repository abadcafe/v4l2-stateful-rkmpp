//! module: image; responsibility: describe borrowed or owned DMA-BUF
//! images. operations: plane layout access, image construction/borrow;
//! entities: Plane, Image. tests: image_tests and quantize_tests via
//! public image views.
use std::os::fd::{AsFd, BorrowedFd};

use crate::dma::Buffer;

#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
  fd: BorrowedFd<'a>,
  stride: u32,
  rows: u32,
}
impl<'a> Plane<'a> {
  pub fn new(fd: BorrowedFd<'a>, stride: u32, rows: u32) -> Self {
    Self { fd, stride, rows }
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
    Plane::new(self.buffer.as_fd(), self.stride, self.rows)
  }
}
