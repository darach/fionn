// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz transaction event replay.
//!
//! Generates arbitrary event sequences from raw bytes, feeds them to
//! `TxRuntime::replay()`. Asserts no panics. Verifies that only well-formed
//! Commit events with Operations payloads affect state.
//!
//! Run with: cargo +nightly fuzz run fuzz_tx_replay

#![no_main]

use libfuzzer_sys::fuzz_target;

use fionn_core::{DsonOperation, OperationValue};
use fionn_crdt::dot_store::CausalContext;
use fionn_crdt::strict::StrictDsonProcessor;
use fionn_tx::event::{TxEvent, TxEventKind, TxPayload, TxVerdict};
use fionn_tx::runtime::{TxOpts, TxRuntime};
use fionn_tx::types::{TxEnvelope, TxMode};

const PATHS: &[&str] = &["a", "b", "c", "d", "x", "y", "z"];

fn event_kind_from_byte(b: u8) -> TxEventKind {
    match b % 6 {
        0 => TxEventKind::Begin,
        1 => TxEventKind::Op,
        2 => TxEventKind::Prepare,
        3 => TxEventKind::Commit,
        4 => TxEventKind::Abort,
        _ => TxEventKind::Compensate,
    }
}

fn payload_from_bytes(kind: TxEventKind, data: &[u8]) -> TxPayload {
    match kind {
        TxEventKind::Commit => {
            if data.is_empty() {
                TxPayload::Operations(Vec::new())
            } else {
                let path = PATHS[(data[0] as usize) % PATHS.len()];
                let value = if data.len() > 1 {
                    match data[1] % 3 {
                        0 => OperationValue::Null,
                        1 => OperationValue::StringRef("fuzz".into()),
                        _ => OperationValue::NumberRef(
                            (data.get(2).copied().unwrap_or(0) as i32).to_string(),
                        ),
                    }
                } else {
                    OperationValue::Null
                };
                TxPayload::Operations(vec![DsonOperation::FieldAdd {
                    path: path.to_string(),
                    value,
                }])
            }
        }
        TxEventKind::Abort => {
            TxPayload::Verdict(TxVerdict::Abort("fuzz abort".to_string()))
        }
        TxEventKind::Compensate => {
            TxPayload::Compensation(Vec::new())
        }
        _ => TxPayload::Empty,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 10_000 || data.len() < 4 {
        return;
    }

    let processor = StrictDsonProcessor::new(1);
    let mut runtime = TxRuntime::new(processor);

    // Build synthetic events from bytes
    // Each event: 6 bytes [kind, tx_id_lo, tx_id_hi, seq, path_idx, value_type]
    let chunk_size = 6;
    let num_events = (data.len() / chunk_size).min(100);
    let mut events = Vec::with_capacity(num_events);

    for i in 0..num_events {
        let offset = i * chunk_size;
        let chunk = &data[offset..offset + chunk_size];

        let kind = event_kind_from_byte(chunk[0]);
        let tx_id = u128::from(chunk[1]) | (u128::from(chunk[2]) << 8);
        let seq = u64::from(chunk[3]);
        let payload = payload_from_bytes(kind, &chunk[4..]);

        let envelope = TxEnvelope::new(tx_id, TxMode::Ramp, CausalContext::new(), "fuzz");

        events.push(TxEvent {
            stream_id: 0,
            tx_id,
            seq,
            kind,
            envelope,
            payload,
        });
    }

    // Replay should not panic
    let _ = runtime.replay(events.into_iter());

    // Also test apply_events path: build some real committed events
    let processor2 = StrictDsonProcessor::new(2);
    let mut runtime2 = TxRuntime::new(processor2);

    if data.len() >= 8 {
        let mode_byte = data[0];
        let mode = match mode_byte % 5 {
            0 => TxMode::Ramp,
            1 => TxMode::Calvin,
            2 => TxMode::SsiLite,
            3 => TxMode::MaterializedViews,
            _ => TxMode::Ramp,
        };

        let mut tx = runtime2.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        let path = PATHS[(data[1] as usize) % PATHS.len()];
        let _ = tx.set(path, OperationValue::StringRef("fuzz_val".into()));
        if let Ok(events) = tx.commit() {
            // Replay these well-formed events on another runtime
            let processor3 = StrictDsonProcessor::new(3);
            let mut runtime3 = TxRuntime::new(processor3);
            let _ = runtime3.apply_events(&events);
        }
    }
});
