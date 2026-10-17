//! module: bitstream::h264::poc; responsibility: derive picture order
//! under H.264 8.2.1. operations: Counter::derive; entities: Counter,
//! Parameters, Picture, Field; tests: poc_tests. State advances once
//! per primary coded picture, never once per slice.
use std::io;
#[derive(Clone)]
pub(super) enum Parameters {
  Lsb(usize),
  Delta { nonreference: i32, bottom: i32, offsets: Vec<i32> },
  FrameNumber,
}
#[derive(Clone, Copy)]
pub(super) enum Field {
  Frame,
  Top,
  Bottom,
}
#[derive(Clone, Copy)]
pub(super) struct Picture {
  pub frame: u32,
  pub frame_bits: usize,
  pub lsb: u32,
  pub delta: [i32; 2],
  pub bottom_delta: i32,
  pub field: Field,
  pub reference: bool,
  pub idr: bool,
  pub reset: bool,
}
#[derive(Clone, Default)]
pub(super) struct Counter {
  msb: i64,
  lsb: i64,
  frame: u32,
  offset: i64,
}
impl Counter {
  pub(super) fn derive(
    &mut self,
    parameters: &Parameters,
    p: Picture,
  ) -> io::Result<i32> {
    if !(4..=16).contains(&p.frame_bits)
      || p.frame >= 1u32 << p.frame_bits
    {
      return Err(invalid());
    }
    let offset = if p.idr {
      0
    } else {
      self.offset
        + if self.frame > p.frame { 1i64 << p.frame_bits } else { 0 }
    };
    let (top, bottom, msb) = match parameters {
      Parameters::Lsb(bits) => {
        if !(4..=16).contains(bits) || p.lsb >= 1u32 << bits {
          return Err(invalid());
        }
        let modulus = 1i64 << bits;
        let lsb = i64::from(p.lsb);
        let (previous_msb, previous_lsb) =
          if p.idr { (0, 0) } else { (self.msb, self.lsb) };
        let msb = previous_msb
          + if lsb < previous_lsb && previous_lsb - lsb >= modulus / 2 {
            modulus
          } else if lsb > previous_lsb
            && lsb - previous_lsb > modulus / 2
          {
            -modulus
          } else {
            0
          };
        let top = msb + lsb;
        let bottom = top
          + if matches!(p.field, Field::Frame) {
            i64::from(p.bottom_delta)
          } else {
            0
          };
        (top, bottom, msb)
      }
      Parameters::Delta { nonreference, bottom, offsets } => {
        if offsets.len() > 255 {
          return Err(invalid());
        }
        let mut number = if offsets.is_empty() {
          0
        } else {
          offset + i64::from(p.frame)
        };
        if !p.reference && number > 0 {
          number -= 1;
        }
        let mut expected = 0;
        if number > 0 {
          let count = offsets.len() as i64;
          let cycle: i64 = offsets.iter().map(|&n| i64::from(n)).sum();
          expected = (number - 1) / count * cycle
            + offsets[..=((number - 1) % count) as usize]
              .iter()
              .map(|&n| i64::from(n))
              .sum::<i64>();
        }
        if !p.reference {
          expected += i64::from(*nonreference);
        }
        let top = expected + i64::from(p.delta[0]);
        let bottom = top
          + i64::from(*bottom)
          + if matches!(p.field, Field::Frame) {
            i64::from(p.delta[1])
          } else {
            0
          };
        (top, bottom, 0)
      }
      Parameters::FrameNumber => {
        let value = if p.idr {
          0
        } else {
          2 * (offset + i64::from(p.frame)) - i64::from(!p.reference)
        };
        (value, value, 0)
      }
    };
    for value in [offset, top, bottom, msb] {
      i32::try_from(value).map_err(|_| invalid())?;
    }
    let order = match p.field {
      Field::Frame => top.min(bottom),
      Field::Top => top,
      Field::Bottom => bottom,
    };
    self.offset = if p.reset { 0 } else { offset };
    self.frame = if p.reset { 0 } else { p.frame };
    if p.reference {
      self.msb = if p.reset { 0 } else { msb };
      self.lsb = if p.reset {
        if matches!(p.field, Field::Bottom) { 0 } else { top - order }
      } else {
        i64::from(p.lsb)
      };
    }
    Ok(order as i32)
  }
}
fn invalid() -> io::Error {
  io::Error::new(
    io::ErrorKind::InvalidData,
    "invalid H264 picture order",
  )
}
