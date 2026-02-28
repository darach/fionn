// SPDX-License-Identifier: MIT OR Apache-2.0
//! Transaction protocol bindings for Python
//!
//! Exposes the 9 fionn-tx transaction modes with a Pythonic API.

use std::sync::Mutex;

use fionn_core::OperationValue;
use fionn_crdt::strict::StrictDsonProcessor;
use fionn_tx::event::{TxEventKind, TxPayload};
use fionn_tx::runtime::{TxOpts, TxRuntime as RustTxRuntime};
use fionn_tx::types::TxMode as RustTxMode;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::compat::options::DumpOptions;
use crate::types::py_to_json;

/// Transaction protocol mode.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxMode {
    /// Read-Atomic Multi-Partition
    Ramp,
    /// RAMP + Compare-And-Swap
    Rola,
    /// Parallel Snapshot Isolation
    Psi,
    /// Transactional Causal Broadcast
    Tcb,
    /// Deterministic execution
    Calvin,
    /// Compensating transactions
    Saga,
    /// Pre-allocated numeric budgets
    EscrowCounters,
    /// Serializable Snapshot Isolation (lite)
    SsiLite,
    /// Atomic base + derived view updates
    MaterializedViews,
}

impl From<TxMode> for RustTxMode {
    fn from(m: TxMode) -> Self {
        match m {
            TxMode::Ramp => Self::Ramp,
            TxMode::Rola => Self::Rola,
            TxMode::Psi => Self::Psi,
            TxMode::Tcb => Self::Tcb,
            TxMode::Calvin => Self::Calvin,
            TxMode::Saga => Self::Saga,
            TxMode::EscrowCounters => Self::EscrowCounters,
            TxMode::SsiLite => Self::SsiLite,
            TxMode::MaterializedViews => Self::MaterializedViews,
        }
    }
}

/// Transaction lifecycle state.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxState {
    /// Transaction is accepting operations
    Active,
    /// Validation in progress
    Preparing,
    /// Successfully committed
    Committed,
    /// Rolled back
    Aborted,
}

/// A single transaction event returned from commit/abort.
#[pyclass]
#[derive(Clone)]
pub struct TxEvent {
    /// Transaction ID.
    #[pyo3(get)]
    pub tx_id: u128,
    /// Event kind as string.
    #[pyo3(get)]
    pub kind: String,
    /// Number of operations in the payload (0 if not an operations payload).
    #[pyo3(get)]
    pub num_operations: usize,
}

#[pymethods]
impl TxEvent {
    fn __repr__(&self) -> String {
        format!(
            "TxEvent(tx_id={}, kind='{}', num_operations={})",
            self.tx_id, self.kind, self.num_operations
        )
    }
}

const fn event_kind_str(kind: TxEventKind) -> &'static str {
    match kind {
        TxEventKind::Begin => "begin",
        TxEventKind::Op => "op",
        TxEventKind::Prepare => "prepare",
        TxEventKind::Commit => "commit",
        TxEventKind::Abort => "abort",
        TxEventKind::Compensate => "compensate",
    }
}

fn rust_event_to_py(event: &fionn_tx::TxEvent) -> TxEvent {
    let num_ops = match &event.payload {
        TxPayload::Operations(ops) => ops.len(),
        TxPayload::Compensation(ops) => ops.len(),
        _ => 0,
    };
    TxEvent {
        tx_id: event.tx_id,
        kind: event_kind_str(event.kind).to_string(),
        num_operations: num_ops,
    }
}

/// Convert a Python value to an `OperationValue`.
fn py_value_to_op(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<OperationValue> {
    let json_val = py_to_json(py, value, None, &DumpOptions::default())?;
    Ok(match json_val {
        serde_json::Value::Null => OperationValue::Null,
        serde_json::Value::Bool(b) => OperationValue::BoolRef(b),
        serde_json::Value::Number(n) => OperationValue::NumberRef(n.to_string()),
        serde_json::Value::String(s) => OperationValue::StringRef(s),
        serde_json::Value::Array(_) => OperationValue::ArrayRef { start: 0, end: 0 },
        serde_json::Value::Object(_) => OperationValue::ObjectRef { start: 0, end: 0 },
    })
}

/// Transaction runtime — manages CRDT processor and transaction protocols.
///
/// # Examples
/// ```python
/// import fionn.ext as fx
///
/// runtime = fx.TxRuntime(replica_id=1)
/// tx = runtime.begin(fx.TxMode.Ramp)
/// tx.set("name", "Alice")
/// tx.set("age", 30)
/// events = tx.commit()
/// print(events)  # [TxEvent(tx_id=1, kind='commit', num_operations=2)]
/// ```
#[pyclass]
pub struct TxRuntime {
    inner: Mutex<RustTxRuntime>,
}

#[allow(clippy::missing_const_for_fn)]
#[pymethods]
impl TxRuntime {
    #[new]
    #[pyo3(signature = (replica_id=1))]
    fn new(replica_id: u64) -> Self {
        let processor = StrictDsonProcessor::new(replica_id);
        Self {
            inner: Mutex::new(RustTxRuntime::new(processor)),
        }
    }

    /// Begin a new transaction.
    ///
    /// Returns a `Transaction` handle for buffering operations before commit.
    #[pyo3(signature = (mode, timeout_ms=None, max_retries=0))]
    fn begin(&self, mode: TxMode, timeout_ms: Option<u64>, max_retries: u32) -> Transaction {
        Transaction {
            mode,
            timeout_ms,
            max_retries,
            operations: Vec::new(),
            reads: Vec::new(),
            state: TxState::Active,
            committed_events: Vec::new(),
        }
    }

    /// Execute a full transaction: begin, apply operations, commit.
    ///
    /// This is the recommended API for most use cases. Operations are provided
    /// as a dict of {path: value} pairs.
    ///
    /// Returns a list of `TxEvent` on success, or raises an error on abort.
    #[pyo3(signature = (mode, operations, timeout_ms=None, max_retries=0))]
    fn execute(
        &self,
        py: Python<'_>,
        mode: TxMode,
        operations: &Bound<'_, PyDict>,
        timeout_ms: Option<u64>,
        max_retries: u32,
    ) -> PyResult<Vec<TxEvent>> {
        let rust_mode: RustTxMode = mode.into();
        let opts = TxOpts {
            mode: rust_mode,
            timeout_ms,
            max_retries,
        };

        let mut runtime = self.inner.lock().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        let mut tx = runtime.begin_tx(&opts);
        for (key, value) in operations.iter() {
            let path: String = key.extract()?;
            let op_val = py_value_to_op(py, &value)?;
            tx.set(&path, op_val).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Set error: {e}"))
            })?;
        }

        let events = tx
            .commit()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;

        Ok(events.iter().map(rust_event_to_py).collect())
    }

    /// Apply remote transaction events to this runtime.
    fn apply_events(&self, events: Vec<TxEventData>) -> PyResult<()> {
        let mut runtime = self.inner.lock().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        // Reconstruct TxEvents from the stored data
        for event_data in &events {
            let rust_events = &event_data.inner;
            runtime.apply_events(rust_events).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Apply error: {e}"))
            })?;
        }
        Ok(())
    }

    /// Commit a transaction handle, returning events.
    fn commit_tx(&self, _py: Python<'_>, transaction: &mut Transaction) -> PyResult<TxEventBundle> {
        if transaction.state != TxState::Active {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Transaction is not active",
            ));
        }

        let rust_mode: RustTxMode = transaction.mode.into();
        let opts = TxOpts {
            mode: rust_mode,
            timeout_ms: transaction.timeout_ms,
            max_retries: transaction.max_retries,
        };

        let mut runtime = self.inner.lock().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        let mut tx = runtime.begin_tx(&opts);

        // Apply buffered operations
        for (path, json_val) in &transaction.operations {
            let op_val = match json_val {
                serde_json::Value::Null => OperationValue::Null,
                serde_json::Value::Bool(b) => OperationValue::BoolRef(*b),
                serde_json::Value::Number(n) => OperationValue::NumberRef(n.to_string()),
                serde_json::Value::String(s) => OperationValue::StringRef(s.clone()),
                serde_json::Value::Array(_) => OperationValue::ArrayRef { start: 0, end: 0 },
                serde_json::Value::Object(_) => OperationValue::ObjectRef { start: 0, end: 0 },
            };
            tx.set(path, op_val).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Set error: {e}"))
            })?;
        }

        // Apply reads
        for path in &transaction.reads {
            let _ = tx.get(path);
        }

        match tx.commit() {
            Ok(events) => {
                transaction.state = TxState::Committed;
                let py_events: Vec<TxEvent> = events.iter().map(rust_event_to_py).collect();
                let bundle = TxEventBundle {
                    events: py_events,
                    inner: events,
                };
                Ok(bundle)
            }
            Err(e) => {
                transaction.state = TxState::Aborted;
                Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "{e}"
                )))
            }
        }
    }

    /// Abort a transaction handle, returning events.
    fn abort_tx(&self, _py: Python<'_>, transaction: &mut Transaction) -> PyResult<Vec<TxEvent>> {
        if transaction.state != TxState::Active {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Transaction is not active",
            ));
        }

        let rust_mode: RustTxMode = transaction.mode.into();
        let opts = TxOpts {
            mode: rust_mode,
            timeout_ms: transaction.timeout_ms,
            max_retries: transaction.max_retries,
        };

        let mut runtime = self.inner.lock().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Lock error: {e}"))
        })?;

        let tx = runtime.begin_tx(&opts);

        match tx.abort() {
            Ok(events) => {
                transaction.state = TxState::Aborted;
                Ok(events.iter().map(rust_event_to_py).collect())
            }
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "{e}"
            ))),
        }
    }

    fn __repr__(&self) -> String {
        "TxRuntime()".to_string()
    }
}

/// Buffered transaction handle.
///
/// Buffer operations with `set()`, `get()`, `delete()`, then commit or abort
/// via the runtime.
///
/// # Examples
/// ```python
/// runtime = fx.TxRuntime(replica_id=1)
/// tx = runtime.begin(fx.TxMode.Calvin)
/// tx.set("key", "value")
/// events = runtime.commit_tx(tx)
/// ```
#[pyclass]
pub struct Transaction {
    mode: TxMode,
    timeout_ms: Option<u64>,
    max_retries: u32,
    operations: Vec<(String, serde_json::Value)>,
    reads: Vec<String>,
    state: TxState,
    committed_events: Vec<TxEvent>,
}

#[allow(clippy::missing_const_for_fn)]
#[pymethods]
impl Transaction {
    /// Set a value at the given path.
    fn set(&mut self, py: Python<'_>, path: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.state != TxState::Active {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Transaction is not active",
            ));
        }
        let json_val = py_to_json(py, value, None, &DumpOptions::default())?;
        self.operations.push((path.to_string(), json_val));
        Ok(())
    }

    /// Record a read at the given path.
    fn get(&mut self, path: &str) -> PyResult<()> {
        if self.state != TxState::Active {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Transaction is not active",
            ));
        }
        self.reads.push(path.to_string());
        Ok(())
    }

    /// Record a delete at the given path.
    fn delete(&mut self, path: &str) -> PyResult<()> {
        if self.state != TxState::Active {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Transaction is not active",
            ));
        }
        self.operations
            .push((path.to_string(), serde_json::Value::Null));
        Ok(())
    }

    /// Get the current state of this transaction.
    #[getter]
    fn state(&self) -> TxState {
        self.state
    }

    /// Get the mode of this transaction.
    #[getter]
    fn mode(&self) -> TxMode {
        self.mode
    }

    /// Get the number of buffered operations.
    #[getter]
    fn num_operations(&self) -> usize {
        self.operations.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "Transaction(mode={:?}, state={:?}, ops={})",
            self.mode,
            self.state,
            self.operations.len()
        )
    }
}

/// A bundle of transaction events that can be passed to `apply_events`.
#[pyclass]
#[derive(Clone)]
pub struct TxEventBundle {
    /// Python-visible events.
    #[pyo3(get)]
    events: Vec<TxEvent>,
    /// Raw Rust events for `apply_events`.
    inner: Vec<fionn_tx::TxEvent>,
}

/// Internal carrier for raw events (used by `apply_events`).
#[pyclass]
#[derive(Clone)]
pub struct TxEventData {
    inner: Vec<fionn_tx::TxEvent>,
}

#[allow(clippy::missing_const_for_fn)]
#[pymethods]
impl TxEventBundle {
    fn __repr__(&self) -> String {
        format!("TxEventBundle(events={})", self.events.len())
    }

    fn __len__(&self) -> usize {
        self.events.len()
    }

    /// Convert to `TxEventData` for passing to `apply_events`.
    fn to_event_data(&self) -> TxEventData {
        TxEventData {
            inner: self.inner.clone(),
        }
    }
}

/// Execute a quick one-shot transaction.
///
/// Convenience function that creates a runtime, executes a single transaction,
/// and returns the events. Useful for simple operations.
///
/// # Arguments
/// * `mode` - Transaction protocol mode
/// * `operations` - Dict of {path: value} pairs
/// * `replica_id` - Optional replica ID (default: 1)
///
/// # Returns
/// List of `TxEvent`
#[pyfunction]
#[pyo3(signature = (mode, operations, replica_id=1))]
pub fn quick_tx(
    py: Python<'_>,
    mode: TxMode,
    operations: &Bound<'_, PyDict>,
    replica_id: u64,
) -> PyResult<Vec<TxEvent>> {
    let runtime = TxRuntime::new(replica_id);
    runtime.execute(py, mode, operations, None, 0)
}

/// List all available transaction modes.
///
/// Returns a list of mode name strings.
#[pyfunction]
pub fn tx_modes() -> Vec<&'static str> {
    vec![
        "Ramp",
        "Rola",
        "Psi",
        "Tcb",
        "Calvin",
        "Saga",
        "EscrowCounters",
        "SsiLite",
        "MaterializedViews",
    ]
}
