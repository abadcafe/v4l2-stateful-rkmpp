//! module: backend::color; responsibility: translate CICP without
//! hiding UAPI gaps. operations: new, primaries, matrix, transfer,
//! range, unrepresented; entities: Color, Component; tests:
//! color_tests, HDR browser acceptance.
#[derive(Clone, Copy)]
enum Component {
  Default,
  Value(u32),
  Unrepresented(u32),
}
impl Component {
  fn value(self) -> u32 {
    match self {
      Self::Value(value) => value,
      _ => 0,
    }
  }
}
pub(super) struct Color {
  primaries: Component,
  matrix: Component,
  transfer: Component,
  range: u32,
}
impl Color {
  pub(super) fn new(
    primaries: u32,
    matrix: u32,
    transfer: u32,
    range: u32,
  ) -> Self {
    use Component::{Default, Unrepresented, Value};
    Self {
      primaries: match primaries {
        0 | 2 => Default,
        1 => Value(3),
        4 => Value(5),
        5 => Value(6),
        6 => Value(1),
        7 => Value(2),
        9 => Value(10),
        11 => Value(12),
        other => Unrepresented(other),
      },
      matrix: match matrix {
        2 => Default,
        1 => Value(2),
        5 | 6 => Value(1),
        7 => Value(8),
        9 => Value(6),
        10 => Value(7),
        other => Unrepresented(other),
      },
      transfer: match transfer {
        0 | 2 => Default,
        1 | 6 | 14 | 15 => Value(1),
        7 => Value(4),
        8 => Value(5),
        13 => Value(2),
        16 => Value(7),
        17 => Value(6),
        other => Unrepresented(other),
      },
      range: match range {
        1 => 2,
        2 => 1,
        _ => 0,
      },
    }
  }

  pub(super) fn primaries(&self) -> u32 {
    self.primaries.value()
  }

  pub(super) fn matrix(&self) -> u32 {
    self.matrix.value()
  }

  pub(super) fn transfer(&self) -> u32 {
    self.transfer.value()
  }

  pub(super) fn range(&self) -> u32 {
    self.range
  }

  pub(super) fn unrepresented(&self) -> Vec<(&'static str, u32)> {
    [
      ("primaries", self.primaries),
      ("matrix", self.matrix),
      ("transfer", self.transfer),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
      if let Component::Unrepresented(value) = value {
        Some((name, value))
      } else {
        None
      }
    })
    .collect()
  }
}
