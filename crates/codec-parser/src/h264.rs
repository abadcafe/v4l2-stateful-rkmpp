//! module: codec_parser::h264; responsibility: validate one
//! client-framed H.264 AU or metadata packet. operations: new,
//! validate; entities: Validator, Identity; tests: h264_tests. Picture
//! boundaries follow 7.4.1.2.4.
use std::collections::BTreeMap;
use std::io;
use std::ptr::NonNull;
use std::rc::Rc;

use super::ffi;
use crate::syntax::bits::Bits;
use crate::syntax::h264::Sequence;
use crate::syntax::parameters::Parameters;
mod marking;
#[cfg(test)]
mod marking_tests;
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
    let mut identity = None;
    let mut markings = Markings::default();
    let mut unit = super::unit::Unit::annexb(bytes.len());
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
      if offset > bytes.len() || bytes[nal.offset as usize] & 0x80 != 0
      {
        return Err(invalid());
      }
      let kind = nal.type_;
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
          let marking = markings.admit(&nal, &slice)?;
          if unit.header.is_none() {
            unit.prefix = nal.sc_offset as usize;
            // PPS belongs to this parser and was checked above. Copy
            // only owned scalar syntax, never its address.
            let pps = unsafe { &*slice.pps };
            unit.header =
              Some(super::header::Header::H264(super::header::H264 {
                sequence: {
                  let sps = unsafe { pps.sequence.as_ref() }
                    .ok_or_else(invalid)?;
                  let id =
                    u32::try_from(sps.id).map_err(|_| invalid())?;
                  sequences
                    .get(&id)
                    .or_else(|| self.sequences.get(&id))
                    .ok_or_else(invalid)?
                    .clone()
                },
                frame: u32::from(slice.frame_num),
                field: (slice.field_pic_flag != 0)
                  .then_some(slice.bottom_field_flag != 0),
                lsb: u32::from(slice.pic_order_cnt_lsb),
                bottom_delta: slice.delta_pic_order_cnt_bottom,
                delta: slice.delta_pic_order_cnt,
                marking: marking.clone(),
              }));
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
    unsafe { ffi::gst_h264_nal_parser_free(self.parser.as_ptr()) };
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid framed H264 access unit",
  )
}
