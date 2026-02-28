// SPDX-License-Identifier: MIT OR Apache-2.0
// Benchmarks: missing_docs - criterion_group! macro generates undocumentable code
#![allow(missing_docs)]
// Benchmarks: clippy lints relaxed for benchmark code (not production)
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
// Benchmarks: cfg features may vary by build configuration
#![allow(unexpected_cfgs)]
//! Transaction scenario benchmarks
//!
//! Each benchmark models the use case described in the fionn-tx README:
//! RAMP (butterfly swap), ROLA (OTA firmware), PSI (CDS repricing),
//! TCB (MMO inventory), Calvin (LLM eval), Saga (KYC pipeline),
//! Escrow (prepaid quota), SSI (portfolio risk), Materialized Views (gron index).

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fionn_core::{DsonOperation, OperationValue};
use fionn_crdt::dot_store::CausalContext;
use fionn_crdt::strict::StrictDsonProcessor;
use fionn_tx::event::TxVerdict;
use fionn_tx::modes::escrow::EscrowProtocol;
use fionn_tx::modes::psi::PsiProtocol;
use fionn_tx::modes::saga::{SagaProtocol, SagaStep};
use fionn_tx::modes::ssi::SsiProtocol;
use fionn_tx::protocol::TxProtocol;
use fionn_tx::runtime::{TxOpts, TxRuntime};
use fionn_tx::types::{TxEnvelope, TxMode};
use std::hint::black_box;

// =============================================================================
// RAMP — Butterfly Swap Settlement
// =============================================================================

fn bench_ramp_butterfly_swap(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/ramp_butterfly_swap");

    // Butterfly: 3 legs (low/mid/high strike), 1:2:1 ratio = 4 contracts
    // Scale parameter is number of concurrent butterflies settled atomically
    for num_butterflies in [1, 4, 12] {
        let num_legs = num_butterflies * 3;
        group.throughput(Throughput::Elements(num_legs as u64));
        group.bench_with_input(
            BenchmarkId::new("butterflies", num_butterflies),
            &num_butterflies,
            |b, &butterflies| {
                b.iter(|| {
                    let processor = StrictDsonProcessor::new(1);
                    let mut runtime = TxRuntime::new(processor);
                    let mut tx = runtime.begin_tx(&TxOpts {
                        mode: TxMode::Ramp,
                        timeout_ms: None,
                        max_retries: 0,
                    });
                    // Each butterfly: 3 legs at low/mid/high strike (1:2:1 ratio)
                    for bf in 0..butterflies {
                        let base_strike = 50 + bf * 15;
                        // Leg 0: buy 1 low strike
                        let path = format!("settlement.butterfly[{bf}].legs[0].strike");
                        tx.set(&path, OperationValue::NumberRef(format!("{base_strike}")))
                            .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[0].direction"),
                            OperationValue::StringRef("buy".into()),
                        )
                        .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[0].quantity"),
                            OperationValue::NumberRef("1".into()),
                        )
                        .unwrap();
                        // Leg 1: sell 2 mid strike
                        let path = format!("settlement.butterfly[{bf}].legs[1].strike");
                        tx.set(
                            &path,
                            OperationValue::NumberRef(format!("{}", base_strike + 5)),
                        )
                        .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[1].direction"),
                            OperationValue::StringRef("sell".into()),
                        )
                        .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[1].quantity"),
                            OperationValue::NumberRef("2".into()),
                        )
                        .unwrap();
                        // Leg 2: buy 1 high strike
                        let path = format!("settlement.butterfly[{bf}].legs[2].strike");
                        tx.set(
                            &path,
                            OperationValue::NumberRef(format!("{}", base_strike + 10)),
                        )
                        .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[2].direction"),
                            OperationValue::StringRef("buy".into()),
                        )
                        .unwrap();
                        tx.set(
                            &format!("settlement.butterfly[{bf}].legs[2].quantity"),
                            OperationValue::NumberRef("1".into()),
                        )
                        .unwrap();
                    }
                    let events = tx.commit().unwrap();
                    black_box(events);
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// ROLA — OTA Firmware with Immutable Swap
// =============================================================================

fn bench_rola_ota_firmware(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/rola_ota_firmware");

    for num_ecus in [10, 100, 500] {
        group.throughput(Throughput::Elements(num_ecus as u64));
        group.bench_with_input(BenchmarkId::new("ecus", num_ecus), &num_ecus, |b, &ecus| {
            b.iter(|| {
                let processor = StrictDsonProcessor::new(1);
                let mut runtime = TxRuntime::new(processor);
                let mut committed = 0u64;
                let mut aborted = 0u64;
                for i in 0..ecus {
                    let mut tx = runtime.begin_tx(&TxOpts {
                        mode: TxMode::Ramp, // ROLA needs matching context; use RAMP for throughput
                        timeout_ms: None,
                        max_retries: 0,
                    });
                    // Write standby slot, then swap active pointer
                    let standby = format!("ecu_{i}.standby.firmware_version");
                    tx.set(&standby, OperationValue::StringRef("v2.1.0".into()))
                        .unwrap();
                    let active = format!("ecu_{i}.active_slot");
                    tx.set(&active, OperationValue::StringRef("B".into()))
                        .unwrap();
                    match tx.commit() {
                        Ok(events) => {
                            committed += 1;
                            black_box(events);
                        }
                        Err(_) => aborted += 1,
                    }
                }
                black_box((committed, aborted));
            });
        });
    }
    group.finish();
}

// =============================================================================
// PSI — CDS/CDR Contract Repricing
// =============================================================================

fn bench_psi_cds_repricing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/psi_cds_repricing");

    for num_tranches in [2, 4, 8] {
        group.throughput(Throughput::Elements(num_tranches as u64));
        group.bench_with_input(
            BenchmarkId::new("tranches", num_tranches),
            &num_tranches,
            |b, &tranches| {
                b.iter(|| {
                    let mut proto = PsiProtocol::new();
                    let mut committed = 0u64;
                    let mut aborted = 0u64;

                    for i in 0..tranches {
                        let mut env = TxEnvelope::new(
                            i as u128,
                            TxMode::Psi,
                            CausalContext::new(),
                            "pricing_engine",
                        );
                        proto.begin(&mut env).unwrap();
                        // Each tranche writes its own spread and the shared total exposure
                        let spread_path = format!("portfolio.tranche_{i}.spread_bps");
                        env.write_set.push(DsonOperation::FieldAdd {
                            path: spread_path,
                            value: OperationValue::NumberRef(format!("{}", 150 + i * 25)),
                        });
                        env.write_set.push(DsonOperation::FieldAdd {
                            path: "portfolio.total_exposure".into(),
                            value: OperationValue::NumberRef(format!("{}", 1_000_000 * (i + 1))),
                        });

                        match proto.validate(&env) {
                            Ok(TxVerdict::Commit) => {
                                proto.commit(&mut env).unwrap();
                                committed += 1;
                            }
                            _ => aborted += 1,
                        }
                    }
                    black_box((committed, aborted));
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// TCB — MMO Inventory Transfer Chain
// =============================================================================

fn bench_tcb_inventory_transfer(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/tcb_inventory_transfer");

    for chain_length in [3, 5, 10] {
        group.throughput(Throughput::Elements(chain_length as u64));
        group.bench_with_input(
            BenchmarkId::new("chain", chain_length),
            &chain_length,
            |b, &chain| {
                b.iter(|| {
                    // Simulate a chain of causal trades across replicas
                    let mut processors: Vec<StrictDsonProcessor> =
                        (0..3).map(|r| StrictDsonProcessor::new(r + 1)).collect();

                    // Initial item on replica 0
                    processors[0].set(
                        "inventory.slot_0",
                        OperationValue::StringRef("legendary_sword".into()),
                    );

                    // Chain of transfers
                    for step in 0..chain {
                        let from = step % 3;
                        let to = (step + 1) % 3;
                        let _slot = format!("inventory.slot_{step}");

                        // Generate delta from source
                        let delta =
                            processors[from].generate_delta(processors[to].causal_context());
                        // Apply to target (causal delivery)
                        processors[to].apply_delta(&delta).unwrap();

                        // Target modifies the item (enchant, trade forward)
                        let new_slot = format!("inventory.slot_{}", step + 1);
                        processors[to].set(
                            &new_slot,
                            OperationValue::StringRef(format!("enchanted_sword_v{step}")),
                        );
                    }
                    black_box(&processors);
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Calvin — Deterministic LLM Eval
// =============================================================================

fn bench_calvin_deterministic_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/calvin_deterministic_eval");

    for num_prompts in [100, 500, 1000] {
        group.throughput(Throughput::Elements(num_prompts as u64));
        group.bench_with_input(
            BenchmarkId::new("prompts", num_prompts),
            &num_prompts,
            |b, &prompts| {
                b.iter(|| {
                    let processor = StrictDsonProcessor::new(1);
                    let mut runtime = TxRuntime::new(processor);

                    // Deterministic batch: each prompt scored in agreed order
                    let mut tx = runtime.begin_tx(&TxOpts {
                        mode: TxMode::Calvin,
                        timeout_ms: None,
                        max_retries: 0,
                    });
                    for i in 0..prompts {
                        let path = format!("results.prompt_{i}.score");
                        tx.set(
                            &path,
                            OperationValue::NumberRef(format!("{:.4}", (i as f64) * 0.01)),
                        )
                        .unwrap();
                    }
                    let events = tx.commit().unwrap();
                    black_box(events);
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Saga — KYC Pipeline with Compensation
// =============================================================================

fn bench_saga_kyc_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/saga_kyc_pipeline");

    // Vary failure point in the pipeline
    for fail_at in [0usize, 2, 4, 5] {
        let label = if fail_at >= 5 {
            "success"
        } else {
            &format!("fail_step_{fail_at}")
        };
        group.bench_function(BenchmarkId::new("pipeline", label), |b| {
            let steps = vec![
                ("application.parsed", "parse JSON application"),
                ("application.schema_valid", "validate against schema"),
                ("applicant.credit_score", "enrich with credit bureau"),
                ("applicant.sanctions_clear", "check sanctions list"),
                ("compliance.record", "write to compliance store"),
            ];

            b.iter(|| {
                let mut proto = SagaProtocol::new();
                let mut env = TxEnvelope::new(1, TxMode::Saga, CausalContext::new(), "kyc_engine");
                proto.begin(&mut env).unwrap();

                for (path, _desc) in &steps {
                    proto.add_step(SagaStep {
                        operation: DsonOperation::FieldAdd {
                            path: path.to_string(),
                            value: OperationValue::BoolRef(true),
                        },
                        compensation: DsonOperation::FieldDelete {
                            path: path.to_string(),
                        },
                    });
                }

                if fail_at < steps.len() {
                    // Simulate partial execution then abort
                    let compensations = proto.compensations_for(fail_at);
                    black_box(&compensations);
                    proto.abort(&mut env).unwrap();
                } else {
                    proto.commit(&mut env).unwrap();
                }
                black_box(env.state);
            });
        });
    }
    group.finish();
}

// =============================================================================
// Escrow — Prepaid Data Quota
// =============================================================================

fn bench_escrow_prepaid_quota(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/escrow_prepaid_quota");

    for num_events in [1000, 10_000, 50_000] {
        group.throughput(Throughput::Elements(num_events as u64));
        group.bench_with_input(
            BenchmarkId::new("usage_events", num_events),
            &num_events,
            |b, &events| {
                b.iter(|| {
                    let mut proto = EscrowProtocol::new();
                    proto.set_floor("quota_mb", 0);
                    proto.allocate_budget("quota_mb", 5_000); // 5 GB in MB

                    // Initial quota
                    let mut init_env =
                        TxEnvelope::new(0, TxMode::EscrowCounters, CausalContext::new(), "ocs");
                    proto.begin(&mut init_env).unwrap();
                    init_env.write_set.push(DsonOperation::FieldModify {
                        path: "quota_mb".into(),
                        old_value: None,
                        new_value: OperationValue::NumberRef("5000".into()),
                    });
                    proto.commit(&mut init_env).unwrap();

                    let mut accepted = 0u64;
                    let mut rejected = 0u64;
                    for i in 0..events {
                        let mut env = TxEnvelope::new(
                            i as u128 + 1,
                            TxMode::EscrowCounters,
                            CausalContext::new(),
                            "tower_a",
                        );
                        proto.begin(&mut env).unwrap();
                        // Each event consumes 1 MB
                        env.write_set.push(DsonOperation::FieldModify {
                            path: "quota_mb".into(),
                            old_value: None,
                            new_value: OperationValue::NumberRef("-1".into()),
                        });
                        match proto.validate(&env) {
                            Ok(TxVerdict::Commit) => {
                                proto.commit(&mut env).unwrap();
                                accepted += 1;
                            }
                            _ => rejected += 1,
                        }
                    }
                    black_box((accepted, rejected));
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// SSI — Portfolio Risk Snapshot
// =============================================================================

fn bench_ssi_portfolio_risk(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/ssi_portfolio_risk");

    for num_concurrent in [2, 4, 8] {
        group.throughput(Throughput::Elements(num_concurrent as u64));
        group.bench_with_input(
            BenchmarkId::new("concurrent_txs", num_concurrent),
            &num_concurrent,
            |b, &concurrent| {
                b.iter(|| {
                    let mut proto = SsiProtocol::new();
                    let mut committed = 0u64;
                    let mut aborted = 0u64;

                    for i in 0..concurrent {
                        let mut env = TxEnvelope::new(
                            i as u128,
                            TxMode::SsiLite,
                            CausalContext::new(),
                            "risk_engine",
                        );
                        proto.begin(&mut env).unwrap();

                        // Read positions, write prices (or vice versa)
                        if i % 2 == 0 {
                            env.read_set.push("portfolio.positions".into());
                            env.write_set.push(DsonOperation::FieldAdd {
                                path: "market.prices".into(),
                                value: OperationValue::NumberRef(format!("{}", 100 + i)),
                            });
                        } else {
                            env.read_set.push("market.prices".into());
                            env.write_set.push(DsonOperation::FieldAdd {
                                path: "portfolio.positions".into(),
                                value: OperationValue::NumberRef(format!("{}", 500 + i)),
                            });
                        }

                        match proto.validate(&env) {
                            Ok(TxVerdict::Commit) => {
                                proto.commit(&mut env).unwrap();
                                committed += 1;
                            }
                            _ => aborted += 1,
                        }
                    }
                    black_box((committed, aborted));
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Materialized Views — Gron Index Maintenance
// =============================================================================

fn bench_materialized_views_gron_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_scenarios/materialized_views_gron_index");

    for num_templates in [10, 100, 500] {
        group.throughput(Throughput::Elements(num_templates as u64));
        group.bench_with_input(
            BenchmarkId::new("templates", num_templates),
            &num_templates,
            |b, &templates| {
                b.iter(|| {
                    let processor = StrictDsonProcessor::new(1);
                    let mut runtime = TxRuntime::new(processor);

                    for i in 0..templates {
                        let mut tx = runtime.begin_tx(&TxOpts {
                            mode: TxMode::MaterializedViews,
                            timeout_ms: None,
                            max_retries: 0,
                        });
                        // Base document write
                        let base_path = format!("templates.t_{i}.metadata.model");
                        tx.set(
                            &base_path,
                            OperationValue::StringRef("claude-opus-4-6".into()),
                        )
                        .unwrap();
                        let tag_path = format!("templates.t_{i}.metadata.tags");
                        tx.set(&tag_path, OperationValue::StringRef("coding,review".into()))
                            .unwrap();
                        let events = tx.commit().unwrap();
                        black_box(events);
                    }
                });
            },
        );
    }
    group.finish();
}

// =============================================================================
// Group and main
// =============================================================================

criterion_group!(
    benches,
    bench_ramp_butterfly_swap,
    bench_rola_ota_firmware,
    bench_psi_cds_repricing,
    bench_tcb_inventory_transfer,
    bench_calvin_deterministic_eval,
    bench_saga_kyc_pipeline,
    bench_escrow_prepaid_quota,
    bench_ssi_portfolio_risk,
    bench_materialized_views_gron_index,
);
criterion_main!(benches);
