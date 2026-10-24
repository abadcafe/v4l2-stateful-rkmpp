//! module: picture::plan; responsibility: carry immutable coded-picture
//! identity and complete complementary-field plans. operations:
//! new, accessors, complete_pair; entities: Id, Part, Plan, Boundary,
//! Output;
//! tests: plan_tests, backend/presentation_tests.
use codec_parser::Limits;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Boundary {
  #[default]
  Continue,
  Drain,
  Discard,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
  pub order: i32,
  pub display: bool,
  pub before: Boundary,
  pub drain_after: bool,
}

#[derive(
  Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct Id(u64);
impl Id {
  pub fn new(sequence: u64) -> Self {
    Self(sequence)
  }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
  #[default]
  Frame,
  FirstField,
  Continuation,
}
#[derive(Clone, Debug, Default)]
pub struct Plan {
  id: Id,
  part: Part,
  references: Vec<Id>,
  limits: Limits,
  reference: bool,
  output: Output,
}
impl Plan {
  pub fn new(
    id: Id,
    part: Part,
    references: Vec<Id>,
    limits: Limits,
    reference: bool,
    output: Output,
  ) -> Self {
    Self { id, part, references, limits, reference, output }
  }

  pub fn id(&self) -> Id {
    self.id
  }

  pub fn part(&self) -> Part {
    self.part
  }

  pub fn references(&self) -> &[Id] {
    &self.references
  }

  pub fn limits(&self) -> Limits {
    self.limits
  }

  pub fn reference(&self) -> bool {
    self.reference
  }

  pub fn output(&self) -> Output {
    self.output
  }

  pub fn complete_pair(
    &mut self,
    second: &Self,
  ) -> std::io::Result<()> {
    if self.id != second.id
      || self.part != Part::FirstField
      || second.part != Part::Continuation
    {
      return Err(std::io::Error::other(
        "invalid codec field continuation",
      ));
    }
    self.references.clone_from(&second.references);
    self.output.order = self.output.order.min(second.output.order);
    Ok(())
  }
}
