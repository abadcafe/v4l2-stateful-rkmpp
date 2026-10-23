//! module: picture; responsibility: publish immutable codec picture
//! contracts. operations/entities: buffering limits, identity, plan and
//! output policy; tests: child contract tests and backend presentation
//! tests.
//! operations: picture identity, geometry and output-plan contracts;
mod plan;
pub use plan::{Id, Part, Plan};
mod output;
pub use output::{Boundary, Output};
mod descriptor;
#[cfg(test)]
mod plan_tests;
pub use descriptor::{Descriptor, Geometry};
