//! module: native ABI; responsibility: publish compiler-derived test
//! wire types. operations: Python module initialization; entities:
//! none; tests: native_tests.
mod batch;
mod control;
mod descriptor;
mod message;
mod pool;
mod record;

use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::wire as raw;

#[pymodule]
fn rkmpp_bridge_uapi(module: &Bound<'_, PyModule>) -> PyResult<()> {
  let py = module.py();
  module.add_class::<message::Message>()?;
  module.add_class::<batch::Batch>()?;
  module.add_class::<descriptor::Descriptor>()?;
  module.add_class::<pool::Pool>()?;
  for (name, value) in [
    ("VERSION", u64::from(raw::RB_VERSION)),
    ("MAX_BATCH", u64::from(raw::RB_BATCH_MAX)),
    ("MAX_BUFFERS", u64::from(raw::RB_MAX_BUFFERS)),
    ("MIN_CAPTURE", u64::from(raw::RB_MIN_CAPTURE)),
    ("ACCEPT", raw::RB_IOCTL_ACCEPT),
    ("EXCHANGE", raw::RB_IOCTL_EXCHANGE),
    ("REGISTER_POOL", raw::RB_IOCTL_REGISTER_POOL),
  ] {
    module.add(name, value)?;
  }
  let enumeration = py.import("enum")?.getattr("IntEnum")?;
  macro_rules! variants {
    ($kind:ident; $($variant:ident => $name:literal),* $(,)?) => {
      vec![$(raw::$kind::$variant),*].into_iter().map(|value| {
        (match value { $(raw::$kind::$variant => $name),* }, value as u32)
      }).collect::<Vec<_>>()
    };
  }
  for (name, values) in [
    (
      "Event",
      variants!(Event;
        Packet => "PACKET", Capture => "CAPTURE", Recycle => "RECYCLE",
        Drain => "DRAIN", Reset => "RESET", Close => "CLOSE", Resume => "RESUME",
      ),
    ),
    (
      "Reply",
      variants!(Reply;
        Layout => "LAYOUT", InputDone => "INPUT_DONE", Frame => "FRAME",
        Eos => "EOS", Stopped => "STOPPED", Failed => "FAILED", Draining => "DRAINING",
      ),
    ),
  ] {
    let members = PyDict::new(py);
    for (variant, value) in values {
      members.set_item(variant, value)?;
    }
    let options = PyDict::new(py);
    options.set_item("module", "rkmpp_bridge_uapi")?;
    module
      .add(name, enumeration.call((name, members), Some(&options))?)?;
  }
  Ok(())
}
