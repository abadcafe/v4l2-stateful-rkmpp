//! module: codec_parser::header; responsibility: carry owned validated
//! picture syntax into ordering semantics. operations/entities: Header,
//! H264, Hevc, Short, Long, Marking, Operation; tests:
//! packet_syntax_tests and codec-specific admission tests. No parser
//! pointers escape admission.
//! operations: owned coded-picture header construction;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
  ForgetShort(u32),
  ForgetLong(u32),
  LongFromShort { difference: u32, index: u32 },
  LongLimit(u32),
  Reset,
  CurrentLong(u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Marking {
  Disposable,
  Idr { discard: bool, long: bool },
  Sliding,
  Adaptive(Vec<Operation>),
}
pub struct H264 {
  pub sequence: std::rc::Rc<crate::syntax::h264::Sequence>,
  pub frame: u32,
  pub field: Option<bool>,
  pub lsb: u32,
  pub bottom_delta: i32,
  pub delta: [i32; 2],
  pub marking: Marking,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Short {
  pub delta: i32,
  pub used: bool,
}
pub struct Long {
  pub lsb: u32,
  pub cycle: Option<u64>,
  pub used: bool,
}
pub struct Hevc {
  pub sequence: std::rc::Rc<crate::syntax::hevc::Sequence>,
  pub kind: u8,
  pub temporal_id: u8,
  pub no_prior: bool,
  pub display: bool,
  pub lsb: u32,
  pub short: Vec<Short>,
  pub long: Vec<Long>,
}
pub enum Header {
  H264(H264),
  Hevc(Hevc),
}
