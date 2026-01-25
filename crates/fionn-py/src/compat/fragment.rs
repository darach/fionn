// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fragment - Embed pre-serialized JSON
//!
//! orjson-compatible Fragment class for embedding pre-serialized JSON
//! without re-parsing or re-escaping.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

/// A fragment of pre-serialized JSON.
///
/// Use Fragment to embed already-serialized JSON within a larger structure
/// without the overhead of re-parsing and re-serializing.
///
/// # Examples
/// ```python
/// >>> import fionn
/// >>> fragment = fionn.Fragment(b'{"nested": true}')
/// >>> fionn.dumps({"outer": fragment})
/// b'{"outer":{"nested": true}}'
/// ```
#[pyclass(frozen)]
#[derive(Clone)]
pub struct Fragment {
    /// The raw JSON bytes
    data: Vec<u8>,
}

#[pymethods]
impl Fragment {
    /// Create a new Fragment from JSON bytes or string.
    ///
    /// # Arguments
    /// * `data` - Pre-serialized JSON as bytes or str
    ///
    /// # Raises
    /// * `TypeError` - If data is not bytes or str
    #[new]
    fn new(data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let bytes = if let Ok(b) = data.cast::<PyBytes>() {
            b.as_bytes().to_vec()
        } else if let Ok(s) = data.cast::<PyString>() {
            s.to_cow()?.as_bytes().to_vec()
        } else {
            return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                "Fragment() argument must be bytes or str",
            ));
        };

        Ok(Self { data: bytes })
    }

    /// Get the raw JSON bytes.
    fn __bytes__(&self, py: Python<'_>) -> Py<PyAny> {
        PyBytes::new(py, &self.data).into()
    }

    /// String representation.
    fn __repr__(&self) -> String {
        let preview = if self.data.len() > 50 {
            format!("{}...", String::from_utf8_lossy(&self.data[..50]))
        } else {
            String::from_utf8_lossy(&self.data).to_string()
        };
        format!("Fragment({preview:?})")
    }

    /// Length of the fragment in bytes.
    const fn __len__(&self) -> usize {
        self.data.len()
    }
}

impl Fragment {
    /// Get the raw bytes for serialization.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}
