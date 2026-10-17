//! module: packet_syntax::av1; responsibility: validate AV1 OBU/frame
//! transactions and reference headers. operations: new, validate;
//! entities: Validator, Pending; tests: av1_tests.
use std::io;
use std::ptr::NonNull;

use super::ffi;
struct Pending {
  header: ffi::GstAV1FrameHeaderOBU,
  next_tile: u32,
}
pub(crate) struct Validator {
  parser: NonNull<ffi::GstAV1Parser>,
  pending: Option<Pending>,
  sequence: Vec<u8>,
  completed: Option<bool>,
}
// SAFETY: the parser is owned by exactly one worker, without shared
// access.
unsafe impl Send for Validator {}
impl Validator {
  pub(crate) fn new() -> io::Result<Self> {
    let parser = NonNull::new(unsafe { ffi::gst_av1_parser_new() })
      .ok_or_else(|| io::Error::other("allocate AV1 parser"))?;
    Ok(Self {
      parser,
      pending: None,
      sequence: Vec::new(),
      completed: None,
    })
  }

  pub(crate) fn configuration(&self) -> Vec<u8> {
    self.sequence.clone()
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
  ) -> io::Result<Vec<super::unit::Unit>> {
    let result = self.packet(bytes);
    if result.is_err() {
      // A failed header must not leave partially updated reference
      // state. Keep the last committed sequence so a later
      // keyframe can recover even when the sender does not repeat
      // its sequence header.
      unsafe { ffi::gst_av1_parser_reset(self.parser.as_ptr(), 0) };
      self.pending = None;
      let sequence = std::mem::take(&mut self.sequence);
      if !sequence.is_empty() {
        self.packet(&sequence)?;
      }
    }
    result
  }

  fn packet(
    &mut self,
    mut bytes: &[u8],
  ) -> io::Result<Vec<super::unit::Unit>> {
    if bytes.is_empty() {
      return Err(invalid());
    }
    let total = bytes.len();
    let mut start = 0;
    let mut ranges: Vec<super::unit::Unit> = Vec::new();
    let mut sequence = None;
    while !bytes.is_empty() {
      if bytes[0] & 0x81 != 0 {
        return Err(invalid());
      }
      let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
      // SAFETY: identify returns a borrowed view of this live input
      // slice. None of the views escapes the synchronous parsing
      // call below.
      let mut obu: ffi::GstAV1OBU = unsafe { std::mem::zeroed() };
      let mut consumed = 0;
      let status = unsafe {
        ffi::gst_av1_parser_identify_one_obu(
          self.parser.as_ptr(),
          bytes.as_ptr(),
          length,
          &mut obu,
          &mut consumed,
        )
      };
      if consumed == 0 || consumed > length {
        return Err(invalid());
      }
      if status == 2 {
        bytes = &bytes[consumed as usize..];
        continue;
      }
      check(status)?;
      self.completed = None;
      self.obu(&mut obu)?;
      if let Some(display) = self.completed.take() {
        let end = total - bytes.len() + consumed as usize;
        ranges.push(super::unit::Unit {
          boundary: None,
          range: start..end,
          display,
        });
        start = end;
      }
      if obu.obu_type == 1 {
        sequence = Some(bytes[..consumed as usize].to_vec());
      }
      bytes = &bytes[consumed as usize..];
    }
    if self.pending.is_some() {
      return Err(invalid());
    }
    if let Some(sequence) = sequence {
      self.sequence = sequence;
    }
    if let Some(last) = ranges.last_mut() {
      last.range.end = total;
    }
    Ok(ranges)
  }

  fn obu(&mut self, obu: &mut ffi::GstAV1OBU) -> io::Result<()> {
    let parser = self.parser.as_ptr();
    // SAFETY: parser is exclusive, output values are initialized C
    // structs, and the OBU data is live for every call. Only owned
    // header values persist.
    unsafe {
      match obu.obu_type {
        1 => {
          if self.pending.is_some() {
            return Err(invalid());
          }
          let mut sequence = std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_sequence_header_obu(
            parser,
            obu,
            &mut sequence,
          ))
        }
        2 => {
          if self.pending.is_some() {
            return Err(invalid());
          }
          check(ffi::gst_av1_parser_parse_temporal_delimiter_obu(
            parser, obu,
          ))
        }
        3 | 7 => {
          let mut header: ffi::GstAV1FrameHeaderOBU =
            std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_frame_header_obu(
            parser,
            obu,
            &mut header,
          ))?;
          if header.show_existing_frame != 0 {
            check(ffi::gst_av1_parser_reference_frame_update(
              parser,
              &mut header,
            ))?;
            self.completed = Some(true);
            Ok(())
          } else {
            if obu.obu_type == 3 {
              self.pending = Some(Pending { header, next_tile: 0 });
            }
            Ok(())
          }
        }
        4 => {
          let mut tiles: ffi::GstAV1TileGroupOBU = std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_tile_group_obu(
            parser, obu, &mut tiles,
          ))?;
          self.tiles(&tiles, obu.obu_size)
        }
        5 => {
          let mut metadata = std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_metadata_obu(
            parser,
            obu,
            &mut metadata,
          ))
        }
        6 => {
          if self.pending.is_some() {
            return Err(invalid());
          }
          let mut frame: ffi::GstAV1FrameOBU = std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_frame_obu(
            parser, obu, &mut frame,
          ))?;
          if frame.tile_group.tile_start_and_end_present_flag != 0 {
            return Err(invalid());
          }
          self.pending =
            Some(Pending { header: frame.frame_header, next_tile: 0 });
          self.tiles(&frame.tile_group, obu.obu_size)?;
          if self.pending.is_some() {
            return Err(invalid());
          }
          Ok(())
        }
        8 => {
          let mut tiles = std::mem::zeroed();
          check(ffi::gst_av1_parser_parse_tile_list_obu(
            parser, obu, &mut tiles,
          ))
        }
        // AV1 requires decoders to ignore reserved OBUs and padding.
        0 | 9..=15 => Ok(()),
        _ => Err(invalid()),
      }
    }
  }

  fn tiles(
    &mut self,
    tiles: &ffi::GstAV1TileGroupOBU,
    size: u32,
  ) -> io::Result<()> {
    let pending = self.pending.as_mut().ok_or_else(invalid)?;
    if u32::from(tiles.tg_start) != pending.next_tile
      || tiles.tg_start > tiles.tg_end
      || u32::from(tiles.tg_end) >= tiles.num_tiles
    {
      return Err(invalid());
    }
    for entry in tiles
      .entry
      .get(tiles.tg_start as usize..=tiles.tg_end as usize)
      .ok_or_else(invalid)?
    {
      if entry.tile_size == 0
        || u64::from(entry.tile_offset) + u64::from(entry.tile_size)
          > u64::from(size)
      {
        return Err(invalid());
      }
    }
    pending.next_tile = u32::from(tiles.tg_end) + 1;
    if pending.next_tile == tiles.num_tiles {
      let mut pending = self.pending.take().unwrap();
      self.completed = Some(pending.header.show_frame != 0);
      check(unsafe {
        ffi::gst_av1_parser_reference_frame_update(
          self.parser.as_ptr(),
          &mut pending.header,
        )
      })?;
    }
    Ok(())
  }
}
impl Drop for Validator {
  fn drop(&mut self) {
    unsafe { ffi::gst_av1_parser_free(self.parser.as_ptr()) };
  }
}
fn check(status: u32) -> io::Result<()> {
  if status == 0 { Ok(()) } else { Err(invalid()) }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid AV1 OBU, frame header or tile boundary",
  )
}
