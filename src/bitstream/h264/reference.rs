//! module: bitstream::h264::reference; responsibility: track H264 DPB
//! reference picture identities through marking operations. operations:
//! apply; entities: Memory, Picture, Operation, State, Entry; tests:
//! reference_tests.
use std::io;
#[derive(Clone, Copy)]
pub(super) enum Operation {
  ForgetShort(u32),
  ForgetLong(u32),
  LongFromShort(u32, u32),
  LongLimit(u32),
  Reset,
  CurrentLong(u32),
}
pub(super) enum Marking {
  Disposable,
  Idr { long: bool },
  Sliding,
  Adaptive(Vec<Operation>),
}
pub(super) struct Picture {
  pub frame: u32,
  pub modulus: u32,
  pub field: Option<bool>,
  pub maximum: u32,
  pub gaps_allowed: bool,
  pub marking: Marking,
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
}
#[derive(Default)]
pub(super) struct Memory {
  entries: Vec<Entry>,
  serial: u64,
  unpaired: Option<(u32, bool, bool, u64)>,
  previous_reference: Option<u32>,
}
fn pic_num(entry: &Entry, current: &Picture) -> i64 {
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
fn long_num(entry: &Entry, current: &Picture) -> Option<u32> {
  entry.long.map(|index| {
    current.field.map_or(index, |bottom| {
      2 * index + u32::from(entry.bottom == bottom)
    })
  })
}
impl Memory {
  pub(super) fn reset(&mut self) {
    self.entries.clear();
    self.unpaired = None;
    self.previous_reference = None;
  }

  pub(super) fn apply(
    &mut self,
    picture: Picture,
  ) -> io::Result<State> {
    if !(16..=65536).contains(&picture.modulus)
      || !picture.modulus.is_power_of_two()
      || picture.frame >= picture.modulus
      || picture.maximum > 16
    {
      return Err(io::Error::other(
        "invalid H264 reference parameters",
      ));
    }
    let reference = !matches!(picture.marking, Marking::Disposable);
    let idr = matches!(picture.marking, Marking::Idr { .. });
    let operations = match &picture.marking {
      Marking::Adaptive(operations) => operations.as_slice(),
      _ => &[],
    };
    let max_pic_num =
      picture.modulus * if picture.field.is_some() { 2 } else { 1 };
    if operations.len() > 64
      || operations.iter().any(|op| match *op {
        Operation::ForgetShort(diff) => diff == 0 || diff > max_pic_num,
        Operation::ForgetLong(number) => number >= 32,
        Operation::LongFromShort(diff, index) => {
          diff == 0 || diff > max_pic_num || index >= 16
        }
        Operation::LongLimit(maximum) => maximum > 16,
        Operation::CurrentLong(index) => index >= 16,
        Operation::Reset => false,
      })
    {
      return Err(io::Error::other(
        "H264 reference marking outside syntax domain",
      ));
    }
    let reset_boundary =
      operations.iter().any(|op| matches!(op, Operation::Reset));
    // H.264 7.4.3: MMCO5 changes frame_num for every subsequent
    // decoding use, including complementary-field identity. Derive
    // the stored value once; marking the current picture still uses
    // its original syntax value below.
    let stored_frame = if reset_boundary { 0 } else { picture.frame };
    if !idr && let Some(previous) = self.previous_reference {
      let mut missing = (previous + 1) % picture.modulus;
      if picture.frame != previous && picture.frame != missing {
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
          self.serial =
            self.serial.checked_add(u64::from(skipped)).ok_or_else(
              || io::Error::other("picture identity exhausted"),
            )?;
          missing = (missing + skipped) % picture.modulus;
        }
        while missing != picture.frame {
          self.slide(missing, picture.modulus, picture.maximum);
          self.serial =
            self.serial.checked_add(1).ok_or_else(|| {
              io::Error::other("picture identity exhausted")
            })?;
          for bottom in [false, true] {
            self.entries.push(Entry {
              id: self.serial,
              frame: missing,
              bottom,
              long: None,
            });
          }
          self.previous_reference = Some(missing);
          missing = (missing + 1) % picture.modulus;
        }
      }
    }
    let mut continuation = false;
    let id = if let Some(bottom) = picture.field {
      if let Some((frame, old_bottom, prior_reference, id)) =
        self.unpaired.take()
        && frame == picture.frame
        && bottom != old_bottom
        && prior_reference == reference
        && !idr
        && !reset_boundary
      {
        continuation = true;
        id
      } else {
        self.serial = self.serial.checked_add(1).ok_or_else(|| {
          io::Error::other("picture identity exhausted")
        })?;
        self.unpaired =
          Some((stored_frame, bottom, reference, self.serial));
        self.serial
      }
    } else {
      self.unpaired = None;
      self.serial = self.serial.checked_add(1).ok_or_else(|| {
        io::Error::other("picture identity exhausted")
      })?;
      self.serial
    };
    let mut current_long = None;
    if idr {
      self.entries.clear();
      current_long =
        matches!(picture.marking, Marking::Idr { long: true })
          .then_some(0);
    } else if reference {
      for operation in operations {
        let current =
          picture.field.map_or(i64::from(picture.frame), |_| {
            2 * i64::from(picture.frame) + 1
          });
        match *operation {
          Operation::ForgetShort(diff) => {
            self.entries.retain(|entry| {
              entry.long.is_some()
                || pic_num(entry, &picture) != current - i64::from(diff)
            })
          }
          Operation::ForgetLong(number) => self
            .entries
            .retain(|entry| long_num(entry, &picture) != Some(number)),
          Operation::LongFromShort(diff, index) => {
            let target = current - i64::from(diff);
            let targets: Vec<u64> = self
              .entries
              .iter()
              .filter(|entry| {
                entry.long.is_none()
                  && pic_num(entry, &picture) == target
              })
              .map(|entry| entry.id)
              .collect();
            self.entries.retain(|entry| {
              entry.long != Some(index) || targets.contains(&entry.id)
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
            self.entries.retain(|entry| {
              entry.long.is_none_or(|index| index < maximum)
            })
          }
          Operation::Reset => {
            self.entries.clear();
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
          });
        }
      }
    }
    if reference {
      self.previous_reference = Some(stored_frame);
    }
    Ok(State { id, continuation, references })
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
