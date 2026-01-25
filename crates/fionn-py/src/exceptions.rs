// SPDX-License-Identifier: MIT OR Apache-2.0
//! orjson-compatible exceptions
//!
//! - `JSONEncodeError` - Subclass of `TypeError`, raised when serialization fails
//! - `JSONDecodeError` - Subclass of `ValueError`, raised when parsing fails

use pyo3::create_exception;

// JSONEncodeError is a subclass of TypeError (matches orjson)
create_exception!(
    fionn,
    JSONEncodeError,
    pyo3::exceptions::PyTypeError,
    "Exception raised when JSON encoding fails.\n\nSubclass of TypeError for orjson compatibility."
);

// JSONDecodeError is a subclass of ValueError (matches orjson)
create_exception!(
    fionn,
    JSONDecodeError,
    pyo3::exceptions::PyValueError,
    "Exception raised when JSON decoding fails.\n\nSubclass of ValueError for orjson compatibility."
);
