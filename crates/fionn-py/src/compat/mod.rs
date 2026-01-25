// SPDX-License-Identifier: MIT OR Apache-2.0
//! orjson-compatible API
//!
//! This module provides drop-in replacement for orjson:
//! - `loads(data)` - Parse JSON bytes to Python object
//! - `dumps(obj, default=None, option=None)` - Serialize Python object to JSON bytes
//! - `Fragment` - Embed pre-serialized JSON
//! - `OPT_*` flags - All 14 orjson option flags

mod dumps_impl;
mod fragment;
mod loads_impl;
pub mod options;

pub use dumps_impl::dumps;
pub use fragment::Fragment;
pub use loads_impl::loads;
pub use options::*;
