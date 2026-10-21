//! module: record; responsibility: define owned Python views of POD
//! records. operations: record macro; entities: none; tests:
//! native_tests.
macro_rules! record {
  ($name:ident, raw::$wire:ident; arrays [$($array:ident),*]; fields { $( $field:ident => $setter:ident : $kind:ty = [$member:ident $([$index:tt])?] ),* $(,)? } methods { $($method:item)* }) => {
    #[pyo3::pyclass(module = "rkmpp_bridge_uapi", skip_from_py_object)]
    #[derive(Clone)]
    pub(crate) struct $name { pub(crate) raw: raw::$wire }
    #[pyo3::pymethods]
    impl $name {
      #[new]
      #[pyo3(signature = (**fields))]
      fn new(fields: Option<&pyo3::Bound<'_, pyo3::types::PyDict>>) -> pyo3::PyResult<Self> {
        use pyo3::types::{PyAnyMethods, PyDictMethods};
        // Exhaustive initialization makes every C field part of this binding.
        // Header additions, removals and type changes must update this schema.
        let mut record = Self { raw: raw::$wire {
          $($member: record!(@zero $kind $(, $index)?),)*
          $($array: bytemuck::Zeroable::zeroed(),)*
        } };
        if let Some(fields) = fields {
          for (key, value) in fields.iter() {
            let key: String = key.extract()?;
            match key.as_str() {
              $(stringify!($field) => record.raw.$member $([$index])? = value.extract::<$kind>()?,)*
              _ => return Err(pyo3::exceptions::PyTypeError::new_err(format!("unknown {} field: {key}", stringify!($name)))),
            }
          }
        }
        Ok(record)
      }
      #[classattr]
      const SIZE: usize = std::mem::size_of::<raw::$wire>();
      #[classattr]
      const ALIGN: usize = std::mem::align_of::<raw::$wire>();
      #[staticmethod]
      fn decode(data: &[u8]) -> Option<Self> {
        bytemuck::try_pod_read_unaligned(data).ok().map(|raw| Self { raw })
      }
      fn clone(&self) -> Self { Clone::clone(self) }
      fn __bytes__<'py>(&self, py: pyo3::Python<'py>) -> pyo3::Bound<'py, pyo3::types::PyBytes> {
        pyo3::types::PyBytes::new(py, bytemuck::bytes_of(&self.raw))
      }
      $(
        #[getter]
        fn $field(&self) -> $kind { self.raw.$member $([$index])? }
        #[setter]
        fn $setter(&mut self, value: $kind) { self.raw.$member $([$index])? = value; }
      )*
      $($method)*
    }
  };
  (@zero $kind:ty) => { <$kind as bytemuck::Zeroable>::zeroed() };
  (@zero $kind:ty, 0) => { [<$kind as bytemuck::Zeroable>::zeroed(); 1] };
}
pub(crate) use record;
