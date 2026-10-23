//! module: reference::identity; responsibility: assign identities to
//! consecutive H.264 coded pictures. operations: check, begin, finish,
//! previous, missing, skip, reset; entities: Identity, Picture,
//! Assigned, Field; tests: identity_tests. No reference storage or
//! marking commands are visible.
use std::io;

use super::super::number::Number;

#[derive(Clone, Copy)]
pub(super) struct Picture {
  pub number: Number,
  pub field: Option<bool>,
  pub reference: bool,
  pub idr: bool,
  pub reset: bool,
}
pub(super) struct Assigned {
  pub id: u64,
  pub continuation: bool,
  pub required_long: Option<u32>,
}
struct Field {
  frame: u32,
  bottom: bool,
  reference: bool,
  id: u64,
  long: Option<u32>,
}
#[derive(Default)]
pub(super) struct Identity {
  serial: u64,
  unpaired: Option<Field>,
  previous: Option<u32>,
}
impl Identity {
  pub(super) fn check(&self, p: Picture) -> io::Result<()> {
    if (p.idr && p.number.value() != 0)
      || (!p.idr
        && self.previous == Some(p.number.value())
        && !(p.reference
          && p.field.is_some_and(|bottom| {
            self.unpaired.as_ref().is_some_and(|prior| {
              prior.reference
                && prior.frame == p.number.value()
                && prior.bottom != bottom
            })
          })))
    {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid H264 frame number repetition or IDR origin",
      ));
    }
    Ok(())
  }

  pub(super) fn begin(&mut self, p: Picture) -> io::Result<Assigned> {
    self.check(p)?;
    if let Some(bottom) = p.field {
      if let Some(prior) = self.unpaired.take()
        && prior.frame == p.number.value()
        && bottom != prior.bottom
        && prior.reference == p.reference
        && !p.idr
        && !p.reset
      {
        return Ok(Assigned {
          id: prior.id,
          continuation: true,
          required_long: prior.long,
        });
      }
      self.skip(1)?;
      self.unpaired = Some(Field {
        frame: if p.reset { 0 } else { p.number.value() },
        bottom,
        reference: p.reference,
        id: self.serial,
        long: None,
      });
    } else {
      self.unpaired = None;
      self.skip(1)?;
    }
    Ok(Assigned {
      id: self.serial,
      continuation: false,
      required_long: None,
    })
  }

  pub(super) fn finish(
    &mut self,
    p: Picture,
    assigned: &Assigned,
    long: Option<u32>,
  ) {
    if let Some(field) = &mut self.unpaired
      && field.id == assigned.id
    {
      field.long = long;
    }
    if p.reference {
      self.previous = Some(if p.reset { 0 } else { p.number.value() });
    }
  }

  pub(super) fn previous(&self) -> Option<u32> {
    self.previous
  }

  pub(super) fn skip(&mut self, count: u32) -> io::Result<()> {
    self.serial = self
      .serial
      .checked_add(u64::from(count))
      .ok_or_else(|| io::Error::other("picture identity exhausted"))?;
    Ok(())
  }

  pub(super) fn missing(&mut self, number: Number) -> io::Result<u64> {
    self.skip(1)?;
    self.previous = Some(number.value());
    Ok(self.serial)
  }

  pub(super) fn reset(&mut self) {
    self.unpaired = None;
    self.previous = None;
  }
}
