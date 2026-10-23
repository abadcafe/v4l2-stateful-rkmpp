//! module: bitstream::h264::reference; responsibility: track H264 DPB
//! reference picture identities through marking operations. operations:
//! reset, apply; entities: Memory, Picture, Marking, State,
//! Entry; tests: reference_tests,
//! reference_identity_tests.
use std::io;

use codec_parser::header::Operation;

use super::number::Number;
mod field;
#[cfg(test)]
mod field_tests;
mod identity;
#[cfg(test)]
mod identity_tests;
use identity::Identity;

pub(super) enum Marking<'a> {
  Disposable,
  Idr { long: bool },
  Sliding,
  Adaptive(&'a [Operation]),
}
pub(super) struct Picture<'a> {
  pub frame: u32,
  pub modulus: u32,
  pub field: Option<bool>,
  pub maximum: u32,
  pub gaps_allowed: bool,
  pub marking: Marking<'a>,
}
pub(super) struct State {
  pub id: u64,
  pub references: Vec<u64>,
  pub continuation: bool,
}
struct Entry {
  id: u64,
  frame: u32,
  bottom: bool,
  long: Option<u32>,
  non_existing: bool,
}
#[derive(Default)]
pub(super) struct Memory {
  entries: Vec<Entry>,
  identity: Identity,
  // Exclusive upper bound (max_long_term_frame_idx_plus1). Zero
  // represents "no long-term frame indices", including after reset.
  long_limit: u32,
}
fn pic_num(entry: &Entry, current: &Picture<'_>) -> i64 {
  let frame = i64::from(entry.frame)
    - if entry.frame > current.frame {
      i64::from(current.modulus)
    } else {
      0
    };
  current.field.map_or(frame, |bottom| {
    frame * 2 + i64::from(entry.bottom == bottom)
  })
}
fn long_num(entry: &Entry, current: &Picture<'_>) -> Option<u32> {
  entry.long.map(|index| {
    current.field.map_or(index, |bottom| {
      2 * index + u32::from(entry.bottom == bottom)
    })
  })
}
fn command_sequence(operations: &[Operation]) -> io::Result<()> {
  // H.264 7.4.3.3: MMCO4/5/6 occur at most once, MMCO5 cannot
  // follow MMCO6, and MMCO5 excludes MMCO1/2/3 anywhere in the list.
  let mut limit = false;
  let mut reset = false;
  let mut current = false;
  let mut target = false;
  let mut invalid = false;
  for operation in operations {
    invalid |= match operation {
      Operation::LongLimit(_) => std::mem::replace(&mut limit, true),
      Operation::Reset => {
        std::mem::replace(&mut reset, true) || current
      }
      Operation::CurrentLong(_) => {
        std::mem::replace(&mut current, true)
      }
      Operation::ForgetShort(_)
      | Operation::ForgetLong(_)
      | Operation::LongFromShort { .. } => {
        target = true;
        false
      }
    };
  }
  if invalid || (reset && target) {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "invalid H264 reference marking command sequence",
    ));
  }
  Ok(())
}
impl Memory {
  pub(super) fn reset(&mut self) {
    self.entries.clear();
    self.identity.reset();
    self.long_limit = 0;
  }

  pub(super) fn apply(
    &mut self,
    picture: Picture<'_>,
  ) -> io::Result<State> {
    if !picture.modulus.is_power_of_two() || picture.maximum > 16 {
      return Err(io::Error::other(
        "invalid H264 reference parameters",
      ));
    }
    let reference = !matches!(picture.marking, Marking::Disposable);
    let idr = matches!(picture.marking, Marking::Idr { .. });
    let number = Number::new(
      picture.frame,
      picture.modulus.trailing_zeros() as usize,
    )?;
    let mut primary = identity::Picture {
      number,
      field: picture.field,
      reference,
      idr,
      reset: false,
    };
    // Identity admission precedes every mutation of DPB storage.
    self.identity.check(primary)?;
    let operations = match &picture.marking {
      Marking::Adaptive(operations) => *operations,
      _ => &[],
    };
    let max_pic_num =
      picture.modulus * if picture.field.is_some() { 2 } else { 1 };
    if operations.len() > 64
      || operations.iter().any(|op| match *op {
        // Canonical header operands store
        // difference_of_pic_nums_minus1. Validate before
        // adding/subtracting the implicit one anywhere.
        Operation::ForgetShort(diff) => diff >= max_pic_num,
        Operation::ForgetLong(number) => number >= 32,
        Operation::LongFromShort { difference, index } => {
          difference >= max_pic_num || index >= 16
        }
        Operation::LongLimit(maximum) => maximum > picture.maximum,
        Operation::CurrentLong(index) => index >= 16,
        Operation::Reset => false,
      })
    {
      return Err(io::Error::other(
        "H264 reference marking outside syntax domain",
      ));
    }
    command_sequence(operations)?;
    let reset_boundary =
      operations.iter().any(|op| matches!(op, Operation::Reset));
    primary.reset = reset_boundary;
    // H.264 7.4.3: MMCO5 changes frame_num for every subsequent
    // decoding use, including complementary-field identity. Derive
    // the stored value once; marking the current picture still uses
    // its original syntax value below.
    let stored_frame = if reset_boundary { 0 } else { picture.frame };
    if !idr && let Some(previous) = self.identity.previous() {
      let mut missing = (previous + 1) % picture.modulus;
      if picture.frame != previous && picture.frame != missing {
        // The checked modulus is a power of two, so wrapping
        // subtraction preserves its cyclic offset without overflow.
        let distance =
          picture.frame.wrapping_sub(previous) % picture.modulus;
        if self.entries.iter().any(|entry| {
          let offset =
            entry.frame.wrapping_sub(previous) % picture.modulus;
          entry.long.is_none() && offset != 0 && offset < distance
        }) {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "H264 gap collides with a live short-term reference",
          ));
        }
        if !picture.gaps_allowed {
          return Err(io::Error::other(
            "H264 reference pictures lost: forbidden frame_num gap",
          ));
        }
        let count =
          (picture.frame + picture.modulus - missing) % picture.modulus;
        let mut long = self
          .entries
          .iter()
          .filter(|entry| entry.long.is_some())
          .map(|entry| entry.id)
          .collect::<Vec<_>>();
        long.sort_unstable();
        long.dedup();
        if long.len() >= picture.maximum.max(1) as usize {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame gap has no evictable reference slot",
          ));
        }
        // After at most maximum synthetic pictures, every old
        // short-term reference has been evicted. Advance
        // discarded identities at once; materialize only the
        // suffix that can survive in the DPB.
        let skipped = count.saturating_sub(picture.maximum.max(1));
        if skipped != 0 {
          self.entries.retain(|entry| entry.long.is_some());
          self.identity.skip(skipped)?;
          missing = (missing + skipped) % picture.modulus;
        }
        while missing != picture.frame {
          self.slide(missing, picture.modulus, picture.maximum);
          let id = self.identity.missing(Number::new(
            missing,
            picture.modulus.trailing_zeros() as usize,
          )?)?;
          for bottom in [false, true] {
            self.entries.push(Entry {
              id,
              frame: missing,
              bottom,
              long: None,
              non_existing: true,
            });
          }
          missing = (missing + 1) % picture.modulus;
        }
      }
    }
    let assigned = self.identity.begin(primary)?;
    let id = assigned.id;
    let continuation = assigned.continuation;
    let mut current_long = None;
    if idr {
      self.entries.clear();
      current_long =
        matches!(picture.marking, Marking::Idr { long: true })
          .then_some(0);
      self.long_limit = u32::from(current_long.is_some());
    } else if reference {
      for operation in operations {
        // H.264 7.4.3.3: MMCO3/6 use the limit at this command,
        // not a fixed syntax bound or the final limit of the picture.
        if let Operation::LongFromShort { index, .. }
        | Operation::CurrentLong(index) = *operation
          && index >= self.long_limit
        {
          return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "H264 long-term index exceeds active limit",
          ));
        }
        let current =
          picture.field.map_or(i64::from(picture.frame), |_| {
            2 * i64::from(picture.frame) + 1
          });
        match *operation {
          Operation::ForgetShort(diff) => {
            self.target(&picture, |entry| {
              entry.long.is_none()
                && pic_num(entry, &picture)
                  == current - i64::from(diff) - 1
            })?;
            self.entries.retain(|entry| {
              entry.long.is_some()
                || pic_num(entry, &picture)
                  != current - i64::from(diff) - 1
            })
          }
          Operation::ForgetLong(number) => {
            self.target(&picture, |entry| {
              long_num(entry, &picture) == Some(number)
            })?;
            self.entries.retain(|entry| {
              long_num(entry, &picture) != Some(number)
            });
          }
          Operation::LongFromShort { difference, index } => {
            let target = current - i64::from(difference) - 1;
            let reference = self.target(&picture, |entry| {
              entry.long.is_none() && pic_num(entry, &picture) == target
            })?;
            // Gap pictures participate in sliding/deletion, but H.264
            // 8.2.5.2(d) forbids promoting them to long-term
            // references.
            if reference.non_existing {
              return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "H264 cannot promote a non-existing reference",
              ));
            }
            // An MMCO3 on one member of a short-term pair requires a
            // same-index assignment for its complement in this syntax,
            // even if a later command removes the promoted field.
            if picture.field.is_some() {
              field::promote(
                field::Promotion {
                  number,
                  target,
                  index,
                  short_complement: self.entries.iter().any(|entry| {
                    entry.id == reference.id
                      && entry.bottom != reference.bottom
                      && entry.long.is_none()
                  }),
                  current_pair: continuation && reference.id == id,
                },
                operations,
              )?;
            }
            let target_id = reference.id;
            if current_long == Some(index) && target_id != id {
              return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "H264 promotion would retire the current picture",
              ));
            }
            self.entries.retain(|entry| {
              entry.long != Some(index) || entry.id == target_id
            });
            for entry in &mut self.entries {
              if entry.long.is_none()
                && pic_num(entry, &picture) == target
              {
                entry.long = Some(index);
              }
            }
          }
          Operation::LongLimit(maximum) => {
            if current_long.is_some_and(|index| index >= maximum) {
              return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "H264 limit would retire the current picture",
              ));
            }
            self.long_limit = maximum;
            self.entries.retain(|entry| {
              entry.long.is_none_or(|index| index < maximum)
            });
          }
          Operation::Reset => {
            self.entries.clear();
            self.long_limit = 0;
          }
          Operation::CurrentLong(index) => {
            self.entries.retain(|entry| {
              entry.long != Some(index) || entry.id == id
            });
            current_long = Some(index);
          }
        }
      }
      if matches!(picture.marking, Marking::Sliding)
        && !self.entries.iter().any(|entry| entry.id == id)
      {
        self.slide(picture.frame, picture.modulus, picture.maximum);
      }
    }
    // Keep the first field's actual assignment even if the second
    // removes it. H.264 7.4.3.3 still requires MMCO6 with the same
    // index.
    field::complete(
      field::Current {
        id,
        field: picture.field.is_some(),
        required_long: assigned.required_long,
        assigned_long: current_long,
      },
      self.entries.iter().map(|entry| field::Reference {
        id: entry.id,
        long: entry.long,
      }),
    )?;
    if picture.maximum == 0 {
      self.entries.clear();
    }
    let mut references: Vec<_> =
      self.entries.iter().map(|entry| entry.id).collect();
    references.sort_unstable();
    references.dedup();
    if reference && picture.maximum != 0 {
      // Adaptive marking must free capacity explicitly. Sliding marking
      // cannot evict long-term pictures. Neither path may grow the DPB
      // beyond the SPS contract; the admission owner resets rejected
      // syntax transactions before accepting another coded picture.
      if let Err(index) = references.binary_search(&id) {
        references.insert(index, id);
      }
      if references.len() > picture.maximum as usize {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "H264 reference marking exceeds SPS capacity",
        ));
      }
      for bottom in [false, true] {
        if picture.field.is_none_or(|field| field == bottom) {
          self.entries.push(Entry {
            id,
            frame: stored_frame,
            bottom,
            long: current_long,
            non_existing: false,
          });
        }
      }
    }
    self.identity.finish(primary, &assigned, current_long);
    Ok(State { id, continuation, references })
  }

  fn target(
    &self,
    picture: &Picture<'_>,
    matches: impl Fn(&Entry) -> bool,
  ) -> io::Result<&Entry> {
    let mut entries =
      self.entries.iter().filter(|entry| matches(entry));
    let first = entries.next().ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidData,
        "H264 reference marking target does not exist",
      )
    })?;
    // In frame mode PicNum/LongTermPicNum must name a frame or
    // complementary pair with both fields still in the required class.
    let complete = picture.field.is_some()
      || entries.next().is_some_and(|second| {
        second.id == first.id && second.bottom != first.bottom
      });
    if !complete || entries.next().is_some() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "H264 reference marking target is not a complete picture",
      ));
    }
    Ok(first)
  }

  fn slide(&mut self, frame: u32, modulus: u32, maximum: u32) {
    // Entries are appended in increasing picture identity order; paired
    // fields share adjacent entries. Retain operations preserve that
    // order.
    while self
      .entries
      .iter()
      .map(|entry| entry.id)
      .fold((None, 0usize), |(previous, count), id| {
        (Some(id), count + usize::from(previous != Some(id)))
      })
      .1
      >= maximum.max(1) as usize
    {
      let oldest = self
        .entries
        .iter()
        .filter(|e| e.long.is_none())
        .min_by_key(|e| {
          i64::from(e.frame)
            - if e.frame > frame { i64::from(modulus) } else { 0 }
        })
        .map(|e| e.id);
      let Some(oldest) = oldest else { break };
      self.entries.retain(|e| e.id != oldest);
    }
  }
}
