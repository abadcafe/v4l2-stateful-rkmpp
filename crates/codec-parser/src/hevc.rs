//! module: codec_parser::hevc; responsibility: validate one
//! client-framed HEVC AU or metadata packet. operations: new, validate;
//! entities: Validator; tests: hevc_tests.
use std::collections::BTreeMap;
use std::io;
use std::ptr::NonNull;
use std::rc::Rc;

use super::ffi;
use crate::syntax::bits::Bits;
use crate::syntax::hevc::Sequence;
use crate::syntax::parameters::Parameters;
mod header;
#[cfg(test)]
mod header_tests;
mod picture;
#[cfg(test)]
mod picture_tests;
use picture::Picture;
pub(super) struct Validator {
  parser: NonNull<ffi::GstH265Parser>,
  sets: Parameters,
  sequences: BTreeMap<u32, Rc<Sequence>>,
}
impl Validator {
  pub(super) fn new() -> io::Result<Self> {
    NonNull::new(unsafe { ffi::gst_h265_parser_new() })
      .map(|parser| Self {
        parser,
        sets: Parameters::default(),
        sequences: BTreeMap::new(),
      })
      .ok_or_else(invalid)
  }

  pub(super) fn configuration(&self) -> Vec<u8> {
    self.sets.configuration()
  }

  pub(super) fn validate(
    &mut self,
    bytes: &[u8],
  ) -> io::Result<super::unit::Unit> {
    let result = self.inspect(bytes);
    if result.is_err() {
      let configuration = self.configuration();
      let mut restored = Self::new()?;
      if !configuration.is_empty() {
        restored.inspect(&configuration)?;
      }
      *self = restored;
    }
    result
  }

  fn inspect(&mut self, bytes: &[u8]) -> io::Result<super::unit::Unit> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
      return Err(invalid());
    }
    let mut changes = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    let mut offset = 0;
    let mut picture = None;
    let mut unit = super::unit::Unit::annexb(bytes.len());
    while offset < bytes.len() {
      if bytes[offset..].iter().all(|&b| b == 0) {
        break;
      }
      let mut nal: ffi::GstH265NalUnit = Default::default();
      let status = unsafe {
        ffi::gst_h265_parser_identify_nalu(
          self.parser.as_ptr(),
          bytes.as_ptr(),
          offset as _,
          bytes.len() as _,
          &mut nal,
        )
      };
      if !matches!(status, 0 | 5)
        || nal.size < 2
        || bytes[offset..nal.sc_offset as usize].iter().any(|&b| b != 0)
      {
        return Err(invalid());
      }
      offset = nal.offset as usize + nal.size as usize;
      if offset > bytes.len()
        || bytes[nal.offset as usize] & 0x80 != 0
        || nal.layer_id != 0
        || nal.temporal_id_plus1 == 0
      {
        return Err(invalid());
      }
      let kind = nal.type_;
      match kind {
        36 => unit.sequence_end(nal.sc_offset as usize)?,
        37 => unit.stream_end(nal.sc_offset as usize)?,
        _ => unit.payload()?,
      }
      match kind {
        0..=31 => {
          let mut slice: ffi::GstH265SliceHdr = Default::default();
          let status = unsafe {
            ffi::gst_h265_parser_parse_slice_hdr(
              self.parser.as_ptr(),
              &mut nal,
              &mut slice,
            )
          };
          let current = Picture::new(&nal, &slice);
          let valid = status == 0
            && !slice.pps.is_null()
            && (slice.header_size as usize) < nal.size as usize * 8
            && match &picture {
              None => {
                slice.first_slice_segment_in_pic_flag != 0
                  && slice.dependent_slice_segment_flag == 0
              }
              Some(previous) => {
                slice.first_slice_segment_in_pic_flag == 0
                  && current
                    .as_ref()
                    .is_ok_and(|current| current.agrees(previous))
              }
            };
          let owned = if valid && picture.is_none() {
            // SAFETY: successful parsing resolves PPS/SPS into this
            // exclusively borrowed parser's live storage. Snapshot
            // them before freeing the slice or mutating the parser.
            let parameters = unsafe { slice.pps.as_ref() }
              .and_then(|pps| unsafe { pps.sps.as_ref() });
            Some(parameters.ok_or_else(invalid).and_then(|sps| {
              let sequence = sequences
                .get(&u32::from(sps.id))
                .or_else(|| self.sequences.get(&u32::from(sps.id)))
                .ok_or_else(invalid)?
                .clone();
              header::own(&nal, &slice, sequence, sps)
            }))
          } else {
            None
          };
          unsafe { ffi::gst_h265_slice_hdr_free(&mut slice) };
          if !valid {
            return Err(invalid());
          }
          if let Some(owned) = owned {
            unit.prefix = nal.sc_offset as usize;
            unit.header = Some(super::header::Header::Hevc(owned?));
          }
          if picture.is_none() {
            picture = Some(current?);
          }
        }
        32..=34 => {
          if picture.is_some()
            || unsafe {
              ffi::gst_h265_parser_parse_nal(
                self.parser.as_ptr(),
                &mut nal,
              )
            } != 0
          {
            return Err(invalid());
          }
          let raw = &bytes[nal.offset as usize..offset];
          let raw = &raw[..raw
            .iter()
            .rposition(|&byte| byte != 0)
            .map_or(0, |index| index + 1)];
          let id = match kind {
            32 => Bits::new(&raw[2..]).get(4)?,
            33 => {
              // The installed codecparser stores
              // max_latency_increase_plus1
              // in u8. Preserve the full normative u32 domain here;
              // ordering consumes this owned snapshot and
              // never reparses the SPS.
              let sequence =
                Rc::new(crate::syntax::hevc::parse(&raw[2..])?);
              let id = sequence.geometry.id;
              sequences.insert(id, sequence);
              id
            }
            34 => Bits::new(&raw[2..]).ue()?,
            _ => unreachable!(),
          };
          changes.insert((kind as u8, id), raw);
        }
        35 | 39 if picture.is_some() => return Err(invalid()),
        _ => {}
      }
    }
    if offset == 0 {
      return Err(invalid());
    }
    self.sets.commit(
      &changes
        .iter()
        .map(|(&(kind, id), &raw)| (kind, id, raw))
        .collect::<Vec<_>>(),
    )?;
    self.sequences.extend(sequences);
    Ok(unit)
  }
}
impl Drop for Validator {
  fn drop(&mut self) {
    unsafe { ffi::gst_h265_parser_free(self.parser.as_ptr()) };
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid framed HEVC access unit",
  )
}
