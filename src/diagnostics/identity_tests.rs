//! module: diagnostics::identity_tests; responsibility: define ELF
//! image identity. operations: Rust test cases; entities: none; tests:
//! public ELF contract.
use std::fs;
use std::path::PathBuf;

use super::identity::Image;

fn fixture(name: &str) -> PathBuf {
  std::env::temp_dir()
    .join(format!("rkmpp-elf-{}-{name}", std::process::id()))
}

#[test]
fn test_current_identifies_the_actually_executing_image() {
  let image = Image::current().unwrap();
  let file =
    Image::read(std::path::Path::new("/proc/self/exe")).unwrap();
  assert_eq!(image.build_id(), file.build_id());
  assert!(!image.build_id().is_empty());
}

#[test]
fn test_read_valid_elf_note_and_replacement_have_distinct_identities() {
  let path = fixture("valid");
  let mut elf = vec![0u8; 148];
  elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
  elf[32..40].copy_from_slice(&64u64.to_le_bytes());
  elf[54..56].copy_from_slice(&56u16.to_le_bytes());
  elf[56..58].copy_from_slice(&1u16.to_le_bytes());
  elf[64..68].copy_from_slice(&4u32.to_le_bytes());
  elf[72..80].copy_from_slice(&120u64.to_le_bytes());
  elf[96..104].copy_from_slice(&28u64.to_le_bytes());
  elf[120..124].copy_from_slice(&4u32.to_le_bytes());
  elf[124..128].copy_from_slice(&12u32.to_le_bytes());
  elf[128..132].copy_from_slice(&3u32.to_le_bytes());
  elf[132..136].copy_from_slice(b"GNU\0");
  elf[136..].copy_from_slice(&[1u8; 12]);
  fs::write(&path, &elf).unwrap();
  assert_eq!(Image::read(&path).unwrap().build_id(), "01".repeat(12));
  elf[136..].fill(2);
  fs::write(&path, &elf).unwrap();
  assert_eq!(Image::read(&path).unwrap().build_id(), "02".repeat(12));
  fs::remove_file(path).unwrap();
}

#[test]
fn test_read_missing_truncated_or_unidentified_image_is_an_error() {
  let path = fixture("invalid");
  assert!(
    Image::read(std::path::Path::new("/nonexistent/rkmpp-image"))
      .is_err()
  );
  for contents in [b"".as_slice(), b"not ELF", b"\x7fELF\x02\x01"] {
    fs::write(&path, contents).unwrap();
    assert!(Image::read(&path).is_err());
  }
  fs::remove_file(path).unwrap();
}

#[test]
fn test_read_rejects_malformed_program_and_note_boundaries() {
  let path = fixture("boundaries");
  let mut elf = vec![0u8; 148];
  elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
  elf[32..40].copy_from_slice(&64u64.to_le_bytes());
  elf[54..56].copy_from_slice(&56u16.to_le_bytes());
  elf[56..58].copy_from_slice(&1u16.to_le_bytes());
  elf[64..68].copy_from_slice(&4u32.to_le_bytes());
  elf[72..80].copy_from_slice(&120u64.to_le_bytes());
  elf[96..104].copy_from_slice(&28u64.to_le_bytes());
  elf[120..124].copy_from_slice(&4u32.to_le_bytes());
  elf[124..128].copy_from_slice(&12u32.to_le_bytes());
  elf[128..132].copy_from_slice(&3u32.to_le_bytes());
  elf[132..136].copy_from_slice(b"GNU\0");
  let cases: [(usize, &[u8]); 9] = [
    (5, &[2]),
    (54, &55u16.to_le_bytes()),
    (56, &0u16.to_le_bytes()),
    (56, &1025u16.to_le_bytes()),
    (32, &u64::MAX.to_le_bytes()),
    (96, &65537u64.to_le_bytes()),
    (120, &u32::MAX.to_le_bytes()),
    (124, &7u32.to_le_bytes()),
    (128, &2u32.to_le_bytes()),
  ];
  for (offset, bytes) in cases {
    let mut bad = elf.clone();
    bad[offset..offset + bytes.len()].copy_from_slice(bytes);
    fs::write(&path, bad).unwrap();
    assert!(Image::read(&path).is_err());
  }
  fs::remove_file(path).unwrap();
}
