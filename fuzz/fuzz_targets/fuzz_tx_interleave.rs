// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz interleaved transaction operations across multiple replicas.
//!
//! Interprets bytes as interleaved actions (commit, deliver) across 2-3
//! replicas. Asserts no panics and atomicity invariant maintained.
//!
//! Run with: cargo +nightly fuzz run fuzz_tx_interleave

#![no_main]

use libfuzzer_sys::fuzz_target;

use fionn_core::OperationValue;
use fionn_crdt::strict::StrictDsonProcessor;
use fionn_tx::event::{TxEventKind, TxPayload};
use fionn_tx::runtime::{TxOpts, TxRuntime};
use fionn_tx::types::TxMode;

const PATHS: &[&str] = &["a", "b", "c", "x", "y"];
const NUM_REPLICAS: usize = 3;

fn mode_from_byte(b: u8) -> TxMode {
    match b % 5 {
        0 => TxMode::Ramp,
        1 => TxMode::Calvin,
        2 => TxMode::SsiLite,
        3 => TxMode::MaterializedViews,
        _ => TxMode::Psi,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 10_000 || data.len() < 4 {
        return;
    }

    // Pick mode from first byte
    let mode = mode_from_byte(data[0]);

    let mut runtimes: Vec<TxRuntime> = (0..NUM_REPLICAS)
        .map(|i| TxRuntime::new(StrictDsonProcessor::new(i as u64 + 1)))
        .collect();
    let mut committed_events: Vec<Vec<fionn_tx::TxEvent>> = vec![Vec::new(); NUM_REPLICAS];
    let mut delivered_up_to: Vec<Vec<usize>> = vec![vec![0; NUM_REPLICAS]; NUM_REPLICAS];

    // Each action: 3 bytes [action_type, arg1, arg2]
    let chunk_size = 3;
    let num_actions = ((data.len() - 1) / chunk_size).min(100);

    for i in 0..num_actions {
        let offset = 1 + i * chunk_size;
        let chunk = &data[offset..offset + chunk_size];

        let action_type = chunk[0] % 2;
        let arg1 = chunk[1] as usize;
        let arg2 = chunk[2] as usize;

        match action_type {
            0 => {
                // Commit a tx on a replica
                let replica = arg1 % NUM_REPLICAS;
                let path = PATHS[arg2 % PATHS.len()];

                let mut tx = runtimes[replica].begin_tx(&TxOpts {
                    mode,
                    timeout_ms: None,
                    max_retries: 0,
                });
                let _ = tx.set(path, OperationValue::StringRef(format!("v_r{replica}")));

                match tx.commit() {
                    Ok(events) => {
                        // Atomicity check: commit event must contain all ops
                        for e in &events {
                            if e.kind == TxEventKind::Commit {
                                if let TxPayload::Operations(ops) = &e.payload {
                                    assert!(
                                        !ops.is_empty(),
                                        "committed tx must have operations"
                                    );
                                }
                            }
                        }
                        committed_events[replica].extend(events);
                    }
                    Err(_) => {
                        // Mode-specific rejection (PSI conflict, etc.) — fine
                    }
                }
            }
            _ => {
                // Deliver events from one replica to another
                let from = arg1 % NUM_REPLICAS;
                let to = arg2 % NUM_REPLICAS;
                if from == to {
                    continue;
                }

                let start = delivered_up_to[to][from];
                if start >= committed_events[from].len() {
                    continue;
                }
                let events: Vec<_> = committed_events[from][start..].to_vec();
                delivered_up_to[to][from] = committed_events[from].len();
                let _ = runtimes[to].apply_events(&events);
            }
        }
    }

    // Final check: no panic means state machine consistency is maintained.
    // Verify all committed tx_ids are unique within each replica.
    for events in &committed_events {
        let mut seen_tx_ids = std::collections::HashSet::new();
        for e in events {
            if e.kind == TxEventKind::Commit {
                assert!(
                    seen_tx_ids.insert(e.tx_id),
                    "duplicate commit for tx_id {}",
                    e.tx_id
                );
            }
        }
    }
});
