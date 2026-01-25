// SPDX-License-Identifier: MIT OR Apache-2.0
//! fionn - Python bindings for the fionn JSON library
//!
//! Drop-in replacement for orjson with extended features:
//! - ISONL streaming (11.9x faster than fastest JSON parser)
//! - JSONL streaming with schema filtering
//! - Multi-format support (YAML, TOML, CSV, ISON, TOON)
//! - Gron path-based exploration
//! - Diff/Patch/Merge operations
//! - CRDT conflict-free merging
//! - Zero-copy tape API

use pyo3::prelude::*;

mod compat;
mod exceptions;
mod types;

pub mod ext;

/// fionn - Fast JSON library with SIMD acceleration
///
/// Drop-in replacement for orjson:
/// ```python
/// import fionn
///
/// # Identical to orjson
/// data = fionn.loads(b'{"name": "Alice"}')
/// output = fionn.dumps(data, option=fionn.OPT_INDENT_2)
/// ```
///
/// Extended features via fionn.ext:
/// ```python
/// import fionn.ext as fx
///
/// # ISONL streaming (11.9x faster than JSONL)
/// for batch in fx.IsonlReader("data.isonl"):
///     process(batch)
/// ```
#[pymodule]
fn fionn(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Version info
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add(
        "__doc__",
        "Fast JSON library with SIMD acceleration - drop-in orjson replacement",
    )?;

    // ==========================================================================
    // orjson-compatible API (drop-in replacement)
    // ==========================================================================

    // Core functions
    m.add_function(wrap_pyfunction!(compat::loads, m)?)?;
    m.add_function(wrap_pyfunction!(compat::dumps, m)?)?;

    // Fragment class for pre-serialized JSON
    m.add_class::<compat::Fragment>()?;

    // Option flags (all 14 orjson OPT_* flags)
    m.add("OPT_APPEND_NEWLINE", compat::OPT_APPEND_NEWLINE)?;
    m.add("OPT_INDENT_2", compat::OPT_INDENT_2)?;
    m.add("OPT_NAIVE_UTC", compat::OPT_NAIVE_UTC)?;
    m.add("OPT_NON_STR_KEYS", compat::OPT_NON_STR_KEYS)?;
    m.add("OPT_OMIT_MICROSECONDS", compat::OPT_OMIT_MICROSECONDS)?;
    m.add(
        "OPT_PASSTHROUGH_DATACLASS",
        compat::OPT_PASSTHROUGH_DATACLASS,
    )?;
    m.add("OPT_PASSTHROUGH_DATETIME", compat::OPT_PASSTHROUGH_DATETIME)?;
    m.add("OPT_PASSTHROUGH_SUBCLASS", compat::OPT_PASSTHROUGH_SUBCLASS)?;
    m.add("OPT_SERIALIZE_DATACLASS", compat::OPT_SERIALIZE_DATACLASS)?;
    m.add("OPT_SERIALIZE_NUMPY", compat::OPT_SERIALIZE_NUMPY)?;
    m.add("OPT_SERIALIZE_UUID", compat::OPT_SERIALIZE_UUID)?;
    m.add("OPT_SORT_KEYS", compat::OPT_SORT_KEYS)?;
    m.add("OPT_STRICT_INTEGER", compat::OPT_STRICT_INTEGER)?;
    m.add("OPT_UTC_Z", compat::OPT_UTC_Z)?;

    // Exceptions (orjson-compatible)
    m.add(
        "JSONEncodeError",
        m.py().get_type::<exceptions::JSONEncodeError>(),
    )?;
    m.add(
        "JSONDecodeError",
        m.py().get_type::<exceptions::JSONDecodeError>(),
    )?;

    // ==========================================================================
    // Extended features submodule (fionn.ext)
    // ==========================================================================

    let ext_module = PyModule::new(m.py(), "ext")?;
    ext::register_ext_module(&ext_module)?;
    m.add_submodule(&ext_module)?;

    // Re-export ext module for `from fionn import ext`
    m.py()
        .import("sys")?
        .getattr("modules")?
        .set_item("fionn.ext", ext_module)?;

    Ok(())
}
