//! module: diagnostics::identity; responsibility: identify a linked ELF
//! image. operations: Image current/read/build_id; entities: Image;
//! tests: identity_tests. The running inode is read through
//! /proc/self/exe.
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub(super) struct Image(String);

impl Image {
  pub(super) fn current() -> io::Result<Self> {
    Self::read(Path::new("/proc/self/exe"))
  }

  pub(super) fn build_id(&self) -> &str {
    &self.0
  }

  pub(super) fn read(path: &Path) -> io::Result<Self> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header)?;
    if &header[..6] != b"\x7fELF\x02\x01" {
      return Err(invalid("expected little-endian ELF64"));
    }
    let offset = u64::from_le_bytes(header[32..40].try_into().unwrap());
    let size = u16::from_le_bytes(header[54..56].try_into().unwrap());
    let count = u16::from_le_bytes(header[56..58].try_into().unwrap());
    if size != 56 || count == 0 || count > 1024 {
      return Err(invalid("invalid ELF program headers"));
    }
    for index in 0..count {
      let position = offset
        .checked_add(u64::from(index) * 56)
        .ok_or_else(|| invalid("ELF offset overflow"))?;
      file.seek(SeekFrom::Start(position))?;
      let mut program = [0u8; 56];
      file.read_exact(&mut program)?;
      if u32::from_le_bytes(program[..4].try_into().unwrap()) != 4 {
        continue;
      }
      let position =
        u64::from_le_bytes(program[8..16].try_into().unwrap());
      let length =
        u64::from_le_bytes(program[32..40].try_into().unwrap());
      if length > 65536 {
        return Err(invalid("ELF note exceeds bound"));
      }
      file.seek(SeekFrom::Start(position))?;
      let mut notes = vec![0; length as usize];
      file.read_exact(&mut notes)?;
      if let Some(identity) = note(&notes)? {
        return Ok(Self(identity));
      }
    }
    Err(invalid("ELF image has no GNU build ID"))
  }
}

fn invalid(message: &str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message)
}

fn note(mut bytes: &[u8]) -> io::Result<Option<String>> {
  while !bytes.is_empty() {
    if bytes.len() < 12 {
      return Err(invalid("truncated ELF note"));
    }
    let name =
      u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let descriptor =
      u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let kind = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let body = 12 + name.next_multiple_of(4);
    let end = body
      .checked_add(descriptor.next_multiple_of(4))
      .ok_or_else(|| invalid("ELF note overflow"))?;
    if end > bytes.len() {
      return Err(invalid("truncated ELF note payload"));
    }
    if name == 4 && &bytes[12..16] == b"GNU\0" && kind == 3 {
      if !(8..=64).contains(&descriptor) {
        return Err(invalid("invalid GNU build ID length"));
      }
      return Ok(Some(
        bytes[body..body + descriptor]
          .iter()
          .map(|value| format!("{value:02x}"))
          .collect(),
      ));
    }
    bytes = &bytes[end..];
  }
  Ok(None)
}
