//! module: codec_parser::av1; responsibility: validate AV1 OBU/frame
//! transactions and reference headers. operations: new, validate,
//! reset; entities: Validator, Pending; tests: av1_tests.
use std::io;
use std::ptr::NonNull;

use super::ffi;
use crate::Configuration;
struct Pending<'a> {
  header: ffi::GstAV1FrameHeaderOBU,
  payload: &'a [u8],
  layer: (u8, u8),
  next_tile: u32,
}
pub(crate) struct Validator {
  parser: NonNull<ffi::GstAV1Parser>,
  sequence: Configuration,
}
impl Validator {
  pub(crate) fn new() -> io::Result<Self> {
    let parser = NonNull::new(unsafe { ffi::gst_av1_parser_new() })
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::OutOfMemory,
          "allocate native AV1 parser",
        )
      })?;
    Ok(Self { parser, sequence: Configuration::default() })
  }

  pub(crate) fn configuration(&self) -> Configuration {
    self.sequence.clone()
  }

  pub(crate) fn validate(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<super::unit::Admission> {
    let result = self.packet(bytes, budget);
    if result.is_err() {
      self.reset()?;
    }
    result
  }

  pub(crate) fn reset(&mut self) -> io::Result<()> {
    unsafe { ffi::gst_av1_parser_reset(self.parser.as_ptr(), 0) };
    let sequence = self.sequence.clone();
    if !sequence.is_empty() {
      self.packet(&sequence, 0)?;
    }
    Ok(())
  }

  fn packet(
    &mut self,
    mut bytes: &[u8],
    budget: usize,
  ) -> io::Result<super::unit::Admission> {
    if bytes.is_empty() {
      return Err(invalid());
    }
    let total = bytes.len();
    let mut start = 0;
    let mut ranges: Vec<super::unit::Unit> = Vec::new();
    let mut sequence = None;
    let mut configures_decoder = false;
    // A framed submission owns its whole unfinished picture. Header
    // borrows cannot escape this call or survive failed admission.
    let mut pending = None;
    while !bytes.is_empty() {
      if bytes[0] & 0x81 != 0 {
        return Err(invalid());
      }
      let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
      // SAFETY: identify returns a borrowed view of this live input
      // slice. None of the views escapes the synchronous parsing
      // call below.
      let mut obu: ffi::GstAV1OBU = Default::default();
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
      let payload_start =
        consumed.checked_sub(obu.obu_size).ok_or_else(invalid)?;
      let payload = &bytes[payload_start as usize..consumed as usize];
      if let Some(display) =
        self.obu(&mut obu, payload, &mut pending)?
      {
        if ranges.len() >= budget {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "coded frame budget exhausted",
          ));
        }
        let end = total - bytes.len() + consumed as usize;
        ranges.push(super::unit::Unit {
          configures_decoder: std::mem::take(&mut configures_decoder),
          prefix: 0,
          header: None,
          boundary: None,
          range: start..end,
          display,
        });
        start = end;
      }
      if obu.obu_type == 1 {
        configures_decoder = true;
        sequence = Some(&bytes[..consumed as usize]);
      }
      bytes = &bytes[consumed as usize..];
    }
    if pending.is_some() {
      return Err(invalid());
    }
    let configures_decoder = sequence.is_some();
    if let Some(sequence) = sequence
      && sequence != self.sequence.as_slice()
    {
      self.sequence = Configuration::new(sequence.to_vec());
    }
    if let Some(last) = ranges.last_mut() {
      last.range.end = total;
    }
    Ok(super::unit::Admission { units: ranges, configures_decoder })
  }

  fn obu<'a>(
    &mut self,
    obu: &mut ffi::GstAV1OBU,
    payload: &'a [u8],
    pending: &mut Option<Pending<'a>>,
  ) -> io::Result<Option<bool>> {
    let parser = self.parser.as_ptr();
    let layer = (obu.header.obu_temporal_id, obu.header.obu_spatial_id);
    // SAFETY: parser is exclusive, output values are initialized C
    // structs, and the OBU data is live for every call. Only owned
    // header values persist.
    match obu.obu_type {
      1 => {
        if pending.is_some() {
          return Err(invalid());
        }
        let mut sequence = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_sequence_header_obu(
            parser,
            obu,
            &mut sequence,
          )
        })?;
        Ok(None)
      }
      2 => {
        if pending.is_some() {
          return Err(invalid());
        }
        check(unsafe {
          ffi::gst_av1_parser_parse_temporal_delimiter_obu(parser, obu)
        })?;
        Ok(None)
      }
      7 => {
        // AV1 6.8.1 requires bit-identical copies of the primary
        // header. A successful independent parse does not prove this.
        let primary = pending.as_ref().ok_or_else(invalid)?;
        if primary.payload != payload || primary.layer != layer {
          return Err(invalid());
        }
        Ok(None)
      }
      3 => {
        if pending.is_some() {
          return Err(invalid());
        }
        let mut header: ffi::GstAV1FrameHeaderOBU = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_frame_header_obu(
            parser,
            obu,
            &mut header,
          )
        })?;
        if header.show_existing_frame != 0 {
          check(unsafe {
            ffi::gst_av1_parser_reference_frame_update(
              parser,
              &mut header,
            )
          })?;
          Ok(Some(true))
        } else {
          *pending =
            Some(Pending { header, payload, layer, next_tile: 0 });
          Ok(None)
        }
      }
      4 => {
        if pending.as_ref().is_none_or(|picture| picture.layer != layer)
        {
          return Err(invalid());
        }
        let mut tiles: ffi::GstAV1TileGroupOBU = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_tile_group_obu(
            parser, obu, &mut tiles,
          )
        })?;
        self.tiles(pending, &tiles, obu.obu_size)
      }
      5 => {
        let mut metadata = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_metadata_obu(
            parser,
            obu,
            &mut metadata,
          )
        })?;
        Ok(None)
      }
      6 => {
        if pending.is_some() {
          return Err(invalid());
        }
        let mut frame: ffi::GstAV1FrameOBU = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_frame_obu(parser, obu, &mut frame)
        })?;
        if frame.tile_group.tile_start_and_end_present_flag != 0 {
          return Err(invalid());
        }
        *pending = Some(Pending {
          header: frame.frame_header,
          payload,
          layer,
          next_tile: 0,
        });
        let completed =
          self.tiles(pending, &frame.tile_group, obu.obu_size)?;
        if pending.is_some() {
          return Err(invalid());
        }
        Ok(completed)
      }
      8 => {
        let mut tiles = Default::default();
        check(unsafe {
          ffi::gst_av1_parser_parse_tile_list_obu(
            parser, obu, &mut tiles,
          )
        })?;
        Ok(None)
      }
      // AV1 requires decoders to ignore reserved OBUs and padding.
      0 | 9..=15 => Ok(None),
      _ => Err(invalid()),
    }
  }

  fn tiles(
    &mut self,
    picture: &mut Option<Pending<'_>>,
    tiles: &ffi::GstAV1TileGroupOBU,
    size: u32,
  ) -> io::Result<Option<bool>> {
    let pending = picture.as_mut().ok_or_else(invalid)?;
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
      let mut pending = picture.take().unwrap();
      check(unsafe {
        ffi::gst_av1_parser_reference_frame_update(
          self.parser.as_ptr(),
          &mut pending.header,
        )
      })?;
      return Ok(Some(pending.header.show_frame != 0));
    }
    Ok(None)
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
