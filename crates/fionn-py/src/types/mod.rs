// SPDX-License-Identifier: MIT OR Apache-2.0
//! Type conversion between Python and JSON
//!
//! Fast paths:
//! - `borrowed_value_to_py` - Direct simd-json `BorrowedValue` to Python (fastest)
//! - `owned_value_to_py` - Direct simd-json `OwnedValue` to Python
//!
//! Compatibility paths:
//! - `json_to_py` - Convert `serde_json::Value` to Python object (slower)
//! - `py_to_json` - Convert Python object to `serde_json::Value`

mod deserialize;
mod serialize;
mod serialize_direct;

pub use deserialize::{json_to_py, owned_value_to_py};
pub use serialize::py_to_json;
pub use serialize_direct::py_to_json_bytes;
