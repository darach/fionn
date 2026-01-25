// SPDX-License-Identifier: MIT OR Apache-2.0
//! Multi-format parsing support
//!
//! Parse and serialize YAML, TOML, CSV, ISON, ISONL, JSONL, and TOON formats.
//! All formats are first-class citizens with full read/write support.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use crate::exceptions::JSONDecodeError;
use crate::types::json_to_py;

/// Auto-detect format and parse to Python object.
///
/// Supports: JSON, JSONL, YAML, TOML, CSV, ISON, ISONL, TOON
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// # Auto-detect from content
/// data = fx.parse('{"name": "Alice"}')  # JSON
/// data = fx.parse('name: Alice')         # YAML
/// data = fx.parse('[section]\nkey = "value"')  # TOML
/// ```
#[pyfunction]
pub fn parse(py: Python<'_>, data: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;
    let trimmed = text.trim();

    // Detect format based on content
    if trimmed.is_empty() {
        return Ok(py.None());
    }

    // JSON: starts with { or [
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return parse_json_internal(py, trimmed);
    }

    // JSONL: multiple JSON objects on separate lines
    if trimmed.contains('\n')
        && trimmed
            .lines()
            .next()
            .is_some_and(|l| l.trim().starts_with('{'))
    {
        return super::jsonl::parse_jsonl(py, data, None);
    }

    // ISONL: contains pipe delimiter with schema pattern (table.name|field:type|...)
    if trimmed.contains('|') && trimmed.contains(':') {
        let first_line = trimmed.lines().next().unwrap_or("");
        if first_line.contains("table.")
            || (first_line.matches('|').count() >= 2 && first_line.contains(':'))
        {
            return super::isonl::parse_isonl(py, data, None);
        }
    }

    // TOML: contains [section] headers or key = value
    if trimmed.contains('[') && trimmed.contains(']') && !trimmed.starts_with('[') {
        // Likely TOML with section headers
        return parse_toml(py, data);
    }
    if trimmed.contains(" = ") || trimmed.contains("= ") {
        return parse_toml(py, data);
    }

    // CSV: contains commas and looks tabular
    if trimmed.contains(',') {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() > 1 {
            let first_commas = lines[0].matches(',').count();
            let second_commas = lines.get(1).map_or(0, |l| l.matches(',').count());
            if first_commas > 0 && first_commas == second_commas {
                return parse_csv(py, data, true);
            }
        }
    }

    // YAML: contains colon-space pattern (key: value)
    if trimmed.contains(": ") || trimmed.contains(":\n") {
        return parse_yaml(py, data);
    }

    // Default: try JSON
    parse_json_internal(py, trimmed)
}

/// Internal JSON parsing helper
fn parse_json_internal(py: Python<'_>, text: &str) -> PyResult<Py<PyAny>> {
    let mut bytes = text.as_bytes().to_vec();
    let value: serde_json::Value = simd_json::from_slice(&mut bytes)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid JSON: {e}")))?;
    json_to_py(py, &value)
}

/// Parse YAML string to Python object.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// data = fx.parse_yaml('''
/// name: Alice
/// age: 30
/// items:
///   - apple
///   - banana
/// ''')
/// print(data["name"])  # Alice
/// ```
#[pyfunction]
pub fn parse_yaml(py: Python<'_>, data: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;

    // Parse YAML to serde_json::Value
    let value: serde_json::Value = serde_yaml::from_str(&text)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid YAML: {e}")))?;

    json_to_py(py, &value)
}

/// Parse TOML string to Python object.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// data = fx.parse_toml('''
/// [server]
/// host = "localhost"
/// port = 8080
///
/// [database]
/// url = "postgres://localhost/db"
/// ''')
/// print(data["server"]["port"])  # 8080
/// ```
#[pyfunction]
pub fn parse_toml(py: Python<'_>, data: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;

    // Parse TOML to toml::Value, then convert to serde_json::Value
    let toml_value: toml::Value = toml::from_str(&text)
        .map_err(|e| PyErr::new::<JSONDecodeError, _>(format!("Invalid TOML: {e}")))?;

    // Convert toml::Value to serde_json::Value
    let json_value = toml_to_json(toml_value);
    json_to_py(py, &json_value)
}

/// Convert `toml::Value` to `serde_json::Value`
fn toml_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

/// Parse CSV string to Python list of dicts.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// data = fx.parse_csv('''
/// name,age,city
/// Alice,30,NYC
/// Bob,25,LA
/// ''')
/// print(data[0]["name"])  # Alice
/// ```
#[pyfunction]
#[pyo3(signature = (data, has_header=true))]
pub fn parse_csv(
    py: Python<'_>,
    data: &Bound<'_, PyString>,
    has_header: bool,
) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;
    let list = PyList::empty(py);

    let mut lines = text.lines().peekable();

    // Get headers
    let headers: Vec<String> = if has_header {
        if let Some(header_line) = lines.next() {
            header_line
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            return Ok(list.into());
        }
    } else {
        // Generate numeric headers
        if let Some(first_line) = lines.peek() {
            let count = first_line.split(',').count();
            (0..count).map(|i| i.to_string()).collect()
        } else {
            return Ok(list.into());
        }
    };

    // Parse data rows
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let dict = PyDict::new(py);
        let values: Vec<&str> = line.split(',').collect();

        for (i, header) in headers.iter().enumerate() {
            let value = values.get(i).map_or("", |s| s.trim());

            // Try to parse as number, bool, or keep as string
            let py_value: Py<PyAny> = if let Ok(n) = value.parse::<i64>() {
                n.into_pyobject(py)?.unbind().into_any()
            } else if let Ok(f) = value.parse::<f64>() {
                f.into_pyobject(py)?.unbind().into_any()
            } else if value == "true" || value == "false" {
                (value == "true")
                    .into_pyobject(py)?
                    .to_owned()
                    .unbind()
                    .into_any()
            } else {
                value.into_pyobject(py)?.unbind().into_any()
            };

            dict.set_item(header, py_value)?;
        }

        list.append(dict)?;
    }

    Ok(list.into())
}

/// Parse ISON string to Python object.
///
/// ISON (Indexed Structured Object Notation) is a compact format with
/// references and schema support.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// data = fx.parse_ison('''
/// table.users id:int name:string
/// 1 Alice
/// 2 Bob
/// ''')
/// ```
#[pyfunction]
pub fn parse_ison(py: Python<'_>, data: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;
    let list = PyList::empty(py);

    let mut lines = text.lines().peekable();

    // Parse header line for schema
    let (field_names, field_types) = if let Some(header) = lines.next() {
        parse_ison_header(header)?
    } else {
        return Ok(list.into());
    };

    // Parse data rows
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let dict = PyDict::new(py);
        let values: Vec<&str> = line.split_whitespace().collect();

        for (i, (name, type_hint)) in field_names.iter().zip(field_types.iter()).enumerate() {
            let value = values.get(i).copied().unwrap_or("");

            let py_value: Py<PyAny> = match type_hint.as_str() {
                "int" => {
                    let n: i64 = value.parse().unwrap_or(0);
                    n.into_pyobject(py)?.unbind().into_any()
                }
                "float" => {
                    let f: f64 = value.parse().unwrap_or(0.0);
                    f.into_pyobject(py)?.unbind().into_any()
                }
                "bool" => {
                    let b = value == "true" || value == "1";
                    b.into_pyobject(py)?.to_owned().unbind().into_any()
                }
                _ => value.into_pyobject(py)?.unbind().into_any(),
            };

            dict.set_item(name, py_value)?;
        }

        list.append(dict)?;
    }

    Ok(list.into())
}

/// Parse ISON header line
fn parse_ison_header(header: &str) -> PyResult<(Vec<String>, Vec<String>)> {
    let parts: Vec<&str> = header.split_whitespace().collect();

    // Skip table name (first part like "table.users")
    let field_parts = if parts.first().is_some_and(|p| p.contains('.')) {
        &parts[1..]
    } else {
        &parts[..]
    };

    let mut names = Vec::new();
    let mut types = Vec::new();

    for part in field_parts {
        if let Some((name, type_hint)) = part.split_once(':') {
            names.push(name.to_string());
            types.push(type_hint.to_string());
        } else {
            names.push(part.to_string());
            types.push("string".to_string());
        }
    }

    Ok((names, types))
}

/// Parse TOON string to Python object.
///
/// TOON (Table Oriented Object Notation) is a whitespace-structured format.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// data = fx.parse_toon('''
/// users
///   id  name   age
///   1   Alice  30
///   2   Bob    25
/// ''')
/// ```
#[pyfunction]
pub fn parse_toon(py: Python<'_>, data: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
    let text = data.to_cow()?;
    let result = PyDict::new(py);

    let mut current_table: Option<String> = None;
    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<Py<PyAny>> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        if indent == 0 {
            // Table name
            if let Some(table_name) = current_table.take() {
                let list = PyList::new(py, &rows)?;
                result.set_item(&table_name, list)?;
                rows.clear();
            }
            current_table = Some(trimmed.to_string());
            headers.clear();
        } else if headers.is_empty() {
            // Header row
            headers = trimmed.split_whitespace().map(String::from).collect();
        } else {
            // Data row
            let values: Vec<&str> = trimmed.split_whitespace().collect();
            let dict = PyDict::new(py);

            for (i, header) in headers.iter().enumerate() {
                let value = values.get(i).copied().unwrap_or("");

                let py_value: Py<PyAny> = if let Ok(n) = value.parse::<i64>() {
                    n.into_pyobject(py)?.unbind().into_any()
                } else if let Ok(f) = value.parse::<f64>() {
                    f.into_pyobject(py)?.unbind().into_any()
                } else if value == "true" || value == "false" {
                    (value == "true")
                        .into_pyobject(py)?
                        .to_owned()
                        .unbind()
                        .into_any()
                } else {
                    value.into_pyobject(py)?.unbind().into_any()
                };

                dict.set_item(header, py_value)?;
            }

            rows.push(dict.into());
        }
    }

    // Add final table
    if let Some(table_name) = current_table {
        let list = PyList::new(py, &rows)?;
        result.set_item(&table_name, list)?;
    }

    Ok(result.into())
}

// =============================================================================
// SERIALIZATION FUNCTIONS
// =============================================================================
//
// NOTE: JSONL and ISONL are first-class formats with dedicated modules:
// - jsonl.rs: JsonlReader, JsonlWriter, parse_jsonl, to_jsonl
// - isonl.rs: IsonlReader, IsonlWriter, parse_isonl, to_isonl, jsonl_to_isonl
//
// They are exported directly from mod.rs - no wrappers needed here.
// =============================================================================

/// Convert Python object to YAML string.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// yaml_str = fx.to_yaml({"name": "Alice", "items": [1, 2, 3]})
/// ```
#[pyfunction]
pub fn to_yaml(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<String> {
    use crate::compat::options::DumpOptions;
    use crate::types::py_to_json;

    let value = py_to_json(py, obj, None, &DumpOptions::default())?;

    serde_yaml::to_string(&value).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("YAML serialization error: {e}"))
    })
}

/// Convert Python object to TOML string.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// toml_str = fx.to_toml({"server": {"host": "localhost", "port": 8080}})
/// ```
#[pyfunction]
pub fn to_toml(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<String> {
    use crate::compat::options::DumpOptions;
    use crate::types::py_to_json;

    let json_value = py_to_json(py, obj, None, &DumpOptions::default())?;

    // Convert serde_json::Value to toml::Value
    let toml_value =
        json_to_toml(&json_value).map_err(PyErr::new::<pyo3::exceptions::PyValueError, _>)?;

    toml::to_string_pretty(&toml_value).map_err(|e| {
        PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("TOML serialization error: {e}"))
    })
}

/// Convert `serde_json::Value` to `toml::Value`
fn json_to_toml(value: &serde_json::Value) -> Result<toml::Value, String> {
    match value {
        serde_json::Value::Null => Err("TOML does not support null values".to_string()),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err("Invalid number".to_string())
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let values: Result<Vec<toml::Value>, String> = arr.iter().map(json_to_toml).collect();
            Ok(toml::Value::Array(values?))
        }
        serde_json::Value::Object(obj) => {
            let mut table = toml::map::Map::new();
            for (k, v) in obj {
                table.insert(k.clone(), json_to_toml(v)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

/// Convert Python list of dicts to CSV string.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// csv_str = fx.to_csv([
///     {"name": "Alice", "age": 30},
///     {"name": "Bob", "age": 25}
/// ])
/// ```
#[pyfunction]
pub fn to_csv(_py: Python<'_>, obj: &Bound<'_, PyList>) -> PyResult<String> {
    let mut output = String::new();

    // Get headers from first row
    let mut headers: Vec<String> = Vec::new();
    if let Some(first) = obj.iter().next() {
        let dict = first.cast::<PyDict>().map_err(|_| {
            PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected list of dicts")
        })?;

        for key in dict.keys() {
            headers.push(key.str()?.to_string());
        }

        // Write header row
        output.push_str(&headers.join(","));
        output.push('\n');
    }

    // Write data rows
    for item in obj.iter() {
        let dict = item
            .cast::<PyDict>()
            .map_err(|_| PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected dict"))?;

        let mut values: Vec<String> = Vec::new();
        for header in &headers {
            if let Some(value) = dict.get_item(header)? {
                let s = value.str()?.to_string();
                // Escape commas and quotes
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    values.push(format!("\"{}\"", s.replace('"', "\"\"")));
                } else {
                    values.push(s);
                }
            } else {
                values.push(String::new());
            }
        }
        output.push_str(&values.join(","));
        output.push('\n');
    }

    Ok(output)
}

/// Convert Python object to ISON string.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// ison_str = fx.to_ison(
///     [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}],
///     table="users",
///     schema=["id:int", "name:string"]
/// )
/// ```
#[pyfunction]
#[pyo3(signature = (obj, table, schema))]
pub fn to_ison(
    _py: Python<'_>,
    obj: &Bound<'_, PyList>,
    table: &str,
    schema: Vec<String>,
) -> PyResult<String> {
    let mut output = String::new();

    // Parse schema
    let parsed_schema: Vec<(String, String)> = schema
        .iter()
        .map(|s| {
            if let Some((name, type_hint)) = s.split_once(':') {
                (name.to_string(), type_hint.to_string())
            } else {
                (s.clone(), "string".to_string())
            }
        })
        .collect();

    // Write header
    output.push_str(&format!("table.{table}"));
    for (name, type_hint) in &parsed_schema {
        output.push_str(&format!(" {name}:{type_hint}"));
    }
    output.push('\n');

    // Write data rows
    for item in obj.iter() {
        let dict = item
            .cast::<PyDict>()
            .map_err(|_| PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected dict"))?;

        let mut values: Vec<String> = Vec::new();
        for (name, _) in &parsed_schema {
            if let Some(value) = dict.get_item(name)? {
                values.push(value.str()?.to_string());
            } else {
                values.push(String::new());
            }
        }
        output.push_str(&values.join(" "));
        output.push('\n');
    }

    Ok(output)
}

/// Convert Python object to TOON string.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// toon_str = fx.to_toon({
///     "users": [
///         {"id": 1, "name": "Alice"},
///         {"id": 2, "name": "Bob"}
///     ]
/// })
/// ```
#[pyfunction]
pub fn to_toon(_py: Python<'_>, obj: &Bound<'_, PyDict>) -> PyResult<String> {
    let mut output = String::new();

    for (key, value) in obj.iter() {
        let table_name = key.str()?.to_string();
        output.push_str(&table_name);
        output.push('\n');

        let list = value.cast::<PyList>().map_err(|_| {
            PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected dict with list values")
        })?;

        // Get headers from first item
        let mut headers: Vec<String> = Vec::new();
        if let Some(first) = list.iter().next() {
            let dict = first.cast::<PyDict>().map_err(|_| {
                PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected list of dicts")
            })?;

            for k in dict.keys() {
                headers.push(k.str()?.to_string());
            }

            // Write header row with indent
            output.push_str("  ");
            output.push_str(&headers.join("  "));
            output.push('\n');
        }

        // Write data rows
        for item in list.iter() {
            let dict = item
                .cast::<PyDict>()
                .map_err(|_| PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected dict"))?;

            let mut values: Vec<String> = Vec::new();
            for header in &headers {
                if let Some(value) = dict.get_item(header)? {
                    values.push(value.str()?.to_string());
                } else {
                    values.push(String::new());
                }
            }
            output.push_str("  ");
            output.push_str(&values.join("  "));
            output.push('\n');
        }
    }

    Ok(output)
}
