//! module: packet_syntax::header; responsibility: carry owned validated
//! picture syntax into ordering semantics. operations/entities: Header,
//! H264, Hevc, Long, Marking, Operation; tests: packet_syntax_tests and
//! codec-specific admission tests. No parser pointers escape admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
  ForgetShort(u32),
  ForgetLong(u32),
  LongFromShort { difference: u32, index: u32 },
  LongLimit(u32),
  Reset,
  CurrentLong(u32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Marking {
  Disposable,
  Idr { discard: bool, long: bool },
  Sliding,
  Adaptive(Vec<Operation>),
}
pub(crate) struct H264 {
  pub pps: u32,
  pub frame: u32,
  pub field: Option<bool>,
  pub lsb: u32,
  pub bottom_delta: i32,
  pub delta: [i32; 2],
  pub marking: Marking,
}
pub(crate) struct Long {
  pub lsb: u32,
  pub cycle: Option<u64>,
}
pub(crate) struct Hevc {
  pub pps: u32,
  pub kind: u8,
  pub temporal_id: u8,
  pub no_prior: bool,
  pub display: bool,
  pub lsb: u32,
  pub short: Vec<i32>,
  pub long: Vec<Long>,
}
pub(crate) enum Header {
  H264(H264),
  Hevc(Hevc),
}
