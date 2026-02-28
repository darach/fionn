// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz the full transaction lifecycle: begin → ops → commit/abort.
//!
//! Interprets arbitrary bytes as a sequence of (mode, path, value, action)
//! tuples. For each, begins a tx, applies operations, commits or aborts.
//! Asserts no panics and state machine consistency.
//!
//! Run with: cargo +nightly fuzz run fuzz_tx_commit

#![no_main]

use libfuzzer_sys::fuzz_target;

use fionn_core::OperationValue;
use fionn_crdt::strict::StrictDsonProcessor;
use fionn_tx::event::TxEventKind;
use fionn_tx::runtime::{TxOpts, TxRuntime};
use fionn_tx::types::TxMode;

const PATHS: &[&str] = &["a", "b", "c", "d", "x", "y", "z"];

fn mode_from_byte(b: u8) -> TxMode {
    match b % 9 {
        0 => TxMode::Ramp,
        1 => TxMode::Rola,
        2 => TxMode::Psi,
        3 => TxMode::Tcb,
        4 => TxMode::Calvin,
        5 => TxMode::Saga,
        6 => TxMode::EscrowCounters,
        7 => TxMode::SsiLite,
        _ => TxMode::MaterializedViews,
    }
}

fn value_from_bytes(b: &[u8]) -> OperationValue {
    if b.is_empty() {
        return OperationValue::Null;
    }
    match b[0] % 3 {
        0 => OperationValue::Null,
        1 => {
            let s = String::from_utf8_lossy(&b[1..b.len().min(6)]);
            OperationValue::StringRef(s.into_owned())
        }
        _ => {
            let n = if b.len() >= 5 {
                i32::from_le_bytes([b[1], b[2], b[3], b[4]])
            } else {
                b[0] as i32
            };
            OperationValue::NumberRef(n.to_string())
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Skip very large inputs
    if data.len() > 10_000 || data.len() < 3 {
        return;
    }

    let processor = StrictDsonProcessor::new(1);
    let mut runtime = TxRuntime::new(processor);

    // Each "instruction" is 8 bytes: [mode, path_idx, value_type, v0, v1, v2, v3, action]
    let chunk_size = 8;
    let num_txs = (data.len() / chunk_size).min(50);

    for i in 0..num_txs {
        let offset = i * chunk_size;
        let chunk = &data[offset..offset + chunk_size];

        let mode = mode_from_byte(chunk[0]);
        let path = PATHS[(chunk[1] as usize) % PATHS.len()];
        let value = value_from_bytes(&chunk[2..7]);
        let should_abort = chunk[7] % 4 == 0; // 25% abort rate

        let mut tx = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });

        // Apply operations
        let _ = tx.set(path, value);

        if should_abort {
            let events = tx.abort();
            // Abort should always succeed
            if let Ok(events) = events {
                for e in &events {
                    assert!(
                        e.kind == TxEventKind::Abort || e.kind == TxEventKind::Compensate,
                        "abort event has unexpected kind: {:?}",
                        e.kind
                    );
                }
            }
        } else {
            // Commit may fail (e.g. PSI conflict, ROLA CAS) — that's fine
            let result = tx.commit();
            if let Ok(events) = result {
                // Verify state machine consistency: commit events present
                assert!(
                    events.iter().any(|e| e.kind == TxEventKind::Commit),
                    "committed tx must have Commit event"
                );
            }
            // Errors are acceptable (mode-specific rejections)
        }
    }
});
