//! module: bitstream::h264::recovery; responsibility: H264
//! recovery-point output eligibility. operations: Point::parse, begin,
//! picture; entities: Point, Recovery; tests: recovery_tests.
use std::io;

use super::number::Number;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Point {
  pub count: u32,
  pub broken: bool,
}
impl Point {
  pub(super) fn parse(data: &[u8]) -> io::Result<Option<Self>> {
    let mut messages = super::super::sei::Messages::new(data);
    let mut point = None;
    while let Some(message) = messages.next()? {
      if message.kind == 6 {
        let mut payload = message.payload;
        let count = payload.ue()?;
        payload.skip(1)?; // exact_match_flag does not alter the boundary.
        let broken = payload.get(1)? != 0;
        payload.skip(2)?;
        point = Some(Self { count, broken });
      }
    }
    Ok(point)
  }
}
#[derive(Default)]
pub(super) struct Recovery {
  target: Option<Number>,
  minimum_poc: Option<i32>,
}
impl Recovery {
  pub(super) fn begin(&mut self, point: Point, frame: Number) {
    self.target = Some(frame.advance(point.count));
    self.minimum_poc = None;
  }

  pub(super) fn picture(
    &mut self,
    frame: Number,
    poc: i32,
    reference: bool,
  ) -> bool {
    if let Some(target) = self.target {
      if frame != target || !reference {
        return false;
      }
      self.target = None;
      self.minimum_poc = Some(poc);
    }
    self.minimum_poc.is_none_or(|minimum| poc >= minimum)
  }
}
