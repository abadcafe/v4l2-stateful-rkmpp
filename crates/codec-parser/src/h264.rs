//! module: codec_parser::h264; responsibility: validate one
//! client-framed H.264 AU or metadata packet. operations: new,
//! validate, reset; entities: Validator, Identity, Pending; tests:
//! h264_tests. Picture boundaries follow 7.4.1.2.4.
use std::collections::BTreeMap;
use std::io;
use std::ptr::NonNull;
use std::rc::Rc;

use super::ffi;
use crate::configuration::{Configuration, Parameters};
use crate::syntax::bits::Bits;
use crate::syntax::h264::Sequence;
mod marking;
#[cfg(test)]
mod marking_tests;
mod nal;
#[cfg(test)]
mod nal_tests;
mod slice;
#[cfg(test)]
mod slice_tests;
use marking::Markings;
#[derive(PartialEq, Eq)]
struct Identity {
  pps: usize,
  frame: u16,
  field: (u8, u8),
  reference: bool,
  idr: Option<u16>,
  lsb: u16,
  delta: (i32, [i32; 2]),
}
struct Pending {
  sequence: Rc<Sequence>,
  frame: u32,
  field: Option<bool>,
  lsb: u32,
  bottom_delta: i32,
  delta: [i32; 2],
}
impl Pending {
  fn complete(
    self,
    marking: super::header::Marking,
  ) -> super::header::H264 {
    super::header::H264 {
      sequence: self.sequence,
      frame: self.frame,
      field: self.field,
      lsb: self.lsb,
      bottom_delta: self.bottom_delta,
      delta: self.delta,
      marking,
    }
  }
}
pub(super) struct Validator {
  parser: NonNull<ffi::GstH264NalParser>,
  sets: Parameters,
  sequences: BTreeMap<u32, Rc<Sequence>>,
}
impl Validator {
  pub(super) fn new() -> io::Result<Self> {
    NonNull::new(unsafe { ffi::gst_h264_nal_parser_new() })
      .map(|parser| Self {
        parser,
        sets: Parameters::default(),
        sequences: BTreeMap::new(),
      })
      .ok_or_else(|| {
        io::Error::new(
          io::ErrorKind::OutOfMemory,
          "allocate native H264 parser",
        )
      })
  }

  pub(super) fn configuration(&self) -> Configuration {
    self.sets.configuration()
  }

  pub(super) fn validate(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<super::unit::Unit> {
    // Inspection only interprets input. Classify its syntax failures
    // before recovery, whose native allocation can fail independently.
    let result = self.inspect(bytes, budget).map_err(|error| {
      io::Error::new(io::ErrorKind::InvalidData, error)
    });
    if result.is_err() {
      self.reset()?;
    }
    result
  }

  pub(super) fn reset(&mut self) -> io::Result<()> {
    let configuration = self.configuration();
    let mut restored = Self::new()?;
    restored.sets = std::mem::take(&mut self.sets);
    if !configuration.is_empty() {
      restored.inspect(&configuration, 0)?;
    }
    *self = restored;
    Ok(())
  }

  fn inspect(
    &mut self,
    bytes: &[u8],
    budget: usize,
  ) -> io::Result<super::unit::Unit> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
      return Err(invalid());
    }
    let mut changes = BTreeMap::new();
    let mut sequences = BTreeMap::new();
    let mut offset = 0;
    let mut identity = None;
    let mut picture: Option<(Pending, Markings)> = None;
    let mut unit = super::unit::Unit::annexb(bytes, 1)?;
    while offset < bytes.len() {
      if bytes[offset..].iter().all(|&b| b == 0) {
        break;
      }
      let mut nal: ffi::GstH264NalUnit = Default::default();
      let status = unsafe {
        ffi::gst_h264_parser_identify_nalu(
          self.parser.as_ptr(),
          bytes.as_ptr(),
          offset as _,
          bytes.len() as _,
          &mut nal,
        )
      };
      if !matches!(status, 0 | 5)
        || nal.size == 0
        || bytes[offset..nal.sc_offset as usize].iter().any(|&b| b != 0)
      {
        return Err(invalid());
      }
      offset = nal.offset as usize + nal.size as usize;
      if offset > bytes.len() {
        return Err(invalid());
      }
      let kind =
        nal::Header::try_from(bytes[nal.offset as usize])?.kind();
      match kind {
        10 => unit.sequence_end(nal.sc_offset as usize)?,
        11 => unit.stream_end(nal.sc_offset as usize)?,
        _ => unit.payload()?,
      }
      match kind {
        1 | 5 => {
          let mut slice: ffi::GstH264SliceHdr = Default::default();
          if unsafe {
            ffi::gst_h264_parser_parse_slice_hdr(
              self.parser.as_ptr(),
              &mut nal,
              &mut slice,
              1,
              1,
            )
          } != 0
            || slice.pps.is_null()
            || slice.header_size as usize >= nal.size as usize * 8
          {
            return Err(invalid());
          }
          // SAFETY: successful native parsing resolves PPS/SPS into
          // this exclusively owned parser. Borrow them only until the
          // next native mutation; retain only owned geometry below.
          let pps =
            unsafe { slice.pps.as_ref() }.ok_or_else(invalid)?;
          let sps =
            unsafe { pps.sequence.as_ref() }.ok_or_else(invalid)?;
          let id = u32::try_from(sps.id).map_err(|_| invalid())?;
          let sequence = sequences
            .get(&id)
            .or_else(|| self.sequences.get(&id))
            .ok_or_else(invalid)?;
          slice::validate(
            &slice,
            sequence.geometry,
            sps.mb_adaptive_frame_field_flag != 0,
          )?;
          let current = Identity {
            pps: slice.pps as usize,
            frame: slice.frame_num,
            field: (slice.field_pic_flag, slice.bottom_field_flag),
            reference: nal.ref_idc != 0,
            idr: (nal.type_ == 5).then_some(slice.idr_pic_id),
            lsb: slice.pic_order_cnt_lsb,
            delta: (
              slice.delta_pic_order_cnt_bottom,
              slice.delta_pic_order_cnt,
            ),
          };
          if identity
            .as_ref()
            .is_some_and(|previous| previous != &current)
          {
            return Err(invalid());
          }
          if let Some((_, markings)) = &mut picture {
            markings.admit(&nal, &slice)?;
          } else {
            let markings = Markings::new(&nal, &slice)?;
            unit.prefix = nal.sc_offset as usize;
            let pending = Pending {
              sequence: sequence.clone(),
              frame: u32::from(slice.frame_num),
              field: (slice.field_pic_flag != 0)
                .then_some(slice.bottom_field_flag != 0),
              lsb: u32::from(slice.pic_order_cnt_lsb),
              bottom_delta: slice.delta_pic_order_cnt_bottom,
              delta: slice.delta_pic_order_cnt,
            };
            picture = Some((pending, markings));
          }

          identity = Some(current);
        }
        7 | 8 => {
          if identity.is_some()
            || unsafe {
              ffi::gst_h264_parser_parse_nal(
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
          let id = if kind == 7 {
            let sequence =
              Rc::new(crate::syntax::h264::parse(&raw[1..])?);
            let id = sequence.geometry.id;
            sequences.insert(id, sequence);
            id
          } else {
            Bits::new(&raw[1..]).ue()?
          };
          changes.insert((kind as u8, id), raw);
        }
        6 | 9 if identity.is_some() => return Err(invalid()),
        2..=4 | 14 | 20 | 21 => return Err(invalid()),
        _ => {}
      }
    }
    if offset == 0 {
      return Err(invalid());
    }
    if budget == 0 && identity.is_some() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "coded frame budget exhausted",
      ));
    }
    self.sets.commit(
      &changes
        .iter()
        .map(|(&(kind, id), &raw)| (kind, id, raw))
        .collect::<Vec<_>>(),
    )?;
    self.sequences.extend(sequences);
    if let Some((pending, markings)) = picture {
      unit.header = Some(super::header::Header::H264(
        pending.complete(markings.finish()),
      ));
    }
    Ok(unit)
  }
}
impl Drop for Validator {
  fn drop(&mut self) {
    unsafe { ffi::gst_h264_nal_parser_free(self.parser.as_ptr()) };
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid framed H264 access unit",
  )
}
