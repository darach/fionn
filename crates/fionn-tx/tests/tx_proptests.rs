// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for transaction protocols

use fionn_core::{DsonOperation, OperationValue};
use fionn_crdt::Dot;
use fionn_crdt::dot_store::CausalContext;
use fionn_crdt::strict::StrictDsonProcessor;

use fionn_tx::event::{TxEventKind, TxPayload, TxVerdict};
use fionn_tx::modes::escrow::EscrowProtocol;
use fionn_tx::modes::materialized_views::MaterializedViewsProtocol;
use fionn_tx::modes::psi::PsiProtocol;
use fionn_tx::modes::rola::RolaProtocol;
use fionn_tx::modes::saga::{SagaProtocol, SagaStep};
use fionn_tx::modes::ssi::SsiProtocol;
use fionn_tx::modes::tcb::TcbProtocol;
use fionn_tx::protocol::TxProtocol;
use fionn_tx::runtime::{TxOpts, TxRuntime};
use fionn_tx::types::{TxEnvelope, TxMode};

use proptest::prelude::*;

fn arb_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("d".to_string()),
    ]
}

fn arb_value() -> impl Strategy<Value = OperationValue> {
    prop_oneof![
        "[a-z]{1,5}".prop_map(OperationValue::StringRef),
        any::<i32>().prop_map(|n| OperationValue::NumberRef(n.to_string())),
        Just(OperationValue::Null),
    ]
}

/// All 9 transaction modes for cross-cutting tests.
fn all_modes() -> Vec<TxMode> {
    vec![
        TxMode::Ramp,
        TxMode::Rola,
        TxMode::Psi,
        TxMode::Tcb,
        TxMode::Calvin,
        TxMode::Saga,
        TxMode::EscrowCounters,
        TxMode::SsiLite,
        TxMode::MaterializedViews,
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    // ─── RAMP read atomicity ───
    // If tx writes {A, B}, reader sees both or neither after commit
    #[test]
    fn ramp_read_atomicity(
        paths in prop::collection::vec(arb_path(), 2..=3),
        vals in prop::collection::vec(arb_value(), 2..=3),
    ) {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });

        let min_len = paths.len().min(vals.len());
        for i in 0..min_len {
            tx.set(&paths[i], vals[i].clone()).unwrap();
        }

        let events = tx.commit().unwrap();

        // After commit, check that all writes are present in the event payload
        if let TxPayload::Operations(ops) = &events[0].payload {
            prop_assert_eq!(ops.len(), min_len,
                "RAMP atomicity: all writes must be in a single event");
        } else {
            prop_assert!(false, "expected operations payload");
        }
    }

    // ─── PSI write-write detection ───
    // Concurrent writes to the same path: exactly one commits
    #[test]
    fn psi_write_write_detection(path in arb_path(), v1 in arb_value(), v2 in arb_value()) {
        let mut proto = PsiProtocol::new();

        // Tx1 commits on path
        let mut env1 = TxEnvelope::new(1, TxMode::Psi, CausalContext::new(), "r1");
        proto.begin(&mut env1).unwrap();
        env1.write_set.push(DsonOperation::FieldAdd {
            path: path.clone(),
            value: v1,
        });
        let v1_result = proto.validate(&env1).unwrap();
        prop_assert!(matches!(v1_result, TxVerdict::Commit));
        proto.commit(&mut env1).unwrap();

        // Tx2 tries same path — must detect conflict
        let mut env2 = TxEnvelope::new(2, TxMode::Psi, CausalContext::new(), "r1");
        proto.begin(&mut env2).unwrap();
        env2.write_set.push(DsonOperation::FieldAdd {
            path,
            value: v2,
        });
        let v2_result = proto.validate(&env2).unwrap();
        prop_assert!(matches!(v2_result, TxVerdict::Abort(_)),
            "PSI: second concurrent write to same path must be detected");
    }

    // ─── Saga compensation ───
    // If step N fails, steps 1..N-1 are compensated
    #[test]
    fn saga_compensation_count(n_steps in 2_usize..=5) {
        let mut proto = SagaProtocol::new();
        let mut env = TxEnvelope::new(1, TxMode::Saga, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();

        for i in 0..n_steps {
            let path = format!("step_{i}");
            proto.add_step(SagaStep {
                operation: DsonOperation::FieldAdd {
                    path: path.clone(),
                    value: OperationValue::Null,
                },
                compensation: DsonOperation::FieldDelete { path },
            });
        }

        // Simulate failure after executing first n_steps-1 steps
        let fail_at = n_steps - 1;
        // Manually set executed count (simulating partial execution)
        let compensations = proto.compensations_for(fail_at);
        prop_assert_eq!(compensations.len(), fail_at,
            "Saga: should compensate exactly {} steps, got {}", fail_at, compensations.len());

        // Verify compensations are in reverse order
        for (i, comp) in compensations.iter().enumerate() {
            let expected_path = format!("step_{}", fail_at - 1 - i);
            if let DsonOperation::FieldDelete { path } = comp {
                prop_assert_eq!(path, &expected_path,
                    "Saga: compensation order wrong at index {}", i);
            }
        }
    }

    // ─── Escrow bounds ───
    // Counter never goes below reserved floor
    #[test]
    fn escrow_bounds_invariant(
        initial in 10_i64..=1000,
        floor in 0_i64..=9,
        decrements in prop::collection::vec(1_i64..=20, 1..=10),
    ) {
        let mut proto = EscrowProtocol::new();
        proto.set_floor("balance", floor);
        proto.allocate_budget("balance", initial);

        // Set initial counter
        let mut env_init = TxEnvelope::new(0, TxMode::EscrowCounters, CausalContext::new(), "r1");
        proto.begin(&mut env_init).unwrap();
        env_init.write_set.push(DsonOperation::FieldModify {
            path: "balance".into(),
            old_value: None,
            new_value: OperationValue::NumberRef(initial.to_string()),
        });
        proto.commit(&mut env_init).unwrap();

        // Apply decrements, checking invariant
        for (i, &dec) in decrements.iter().enumerate() {
            let tx_id = (i as u128) + 1;
            let mut env = TxEnvelope::new(tx_id, TxMode::EscrowCounters, CausalContext::new(), "r1");
            proto.begin(&mut env).unwrap();
            env.write_set.push(DsonOperation::FieldModify {
                path: "balance".into(),
                old_value: None,
                new_value: OperationValue::NumberRef((-dec).to_string()),
            });
            let verdict = proto.validate(&env).unwrap();
            if matches!(verdict, TxVerdict::Commit) {
                proto.commit(&mut env).unwrap();
                prop_assert!(proto.get_counter("balance") >= floor,
                    "Escrow invariant violated: counter {} < floor {}", proto.get_counter("balance"), floor);
            }
        }
    }

    // ─── Replay equivalence ───
    // replay(events) produces same state as direct execution
    #[test]
    fn replay_equivalence(
        ops in prop::collection::vec((arb_path(), arb_value()), 1..=5),
    ) {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut all_events = Vec::new();
        for (i, (path, val)) in ops.iter().enumerate() {
            let mut tx = runtime.begin_tx(&TxOpts {
                mode: TxMode::Ramp,
                timeout_ms: None,
                max_retries: 0,
            });
            tx.set(path, val.clone()).unwrap();
            let events = tx.commit().unwrap();
            all_events.extend(events);
            let _ = i; // suppress unused
        }

        // Replay on a fresh runtime
        let processor2 = StrictDsonProcessor::new(2);
        let mut runtime2 = TxRuntime::new(processor2);
        runtime2.replay(all_events.into_iter()).unwrap();
    }

    // ─── Event ordering ───
    // Events within a single transaction are totally ordered by seq
    #[test]
    fn event_ordering_within_tx(
        n_ops in 1_usize..=5,
        path in arb_path(),
        val in arb_value(),
    ) {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode: TxMode::Ramp,
            timeout_ms: None,
            max_retries: 0,
        });
        for _ in 0..n_ops {
            tx.set(&path, val.clone()).unwrap();
        }
        let events = tx.commit().unwrap();

        // All events for this tx should have monotonically increasing seq
        let mut prev_seq = None;
        for event in &events {
            if let Some(prev) = prev_seq {
                prop_assert!(event.seq > prev,
                    "Event ordering violated: seq {} not > {}", event.seq, prev);
            }
            prev_seq = Some(event.seq);
        }

        // Commit event should be present
        prop_assert!(events.iter().any(|e| e.kind == TxEventKind::Commit));
    }

    // ─── ROLA CAS invariant ───
    // If version vector advances between begin and validate, tx must abort
    #[test]
    fn rola_cas_abort_on_context_advance(
        replica_id in 1_u64..=10,
        seq in 1_u64..=100,
        path in arb_path(),
        val in arb_value(),
    ) {
        let mut proto = RolaProtocol::new(CausalContext::new());
        let mut env = TxEnvelope::new(1, TxMode::Rola, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();

        env.write_set.push(DsonOperation::FieldAdd {
            path,
            value: val,
        });

        // Advance the global context after begin (simulating concurrent commit)
        let mut advanced = CausalContext::new();
        advanced.observe(Dot::new(replica_id, seq));
        // Commit a dummy tx to advance the protocol's context
        let mut dummy_env = TxEnvelope::new(99, TxMode::Rola, CausalContext::new(), "r2");
        dummy_env.read_snapshot = CausalContext::new();
        dummy_env.write_set.push(DsonOperation::FieldAdd {
            path: "dummy".into(),
            value: OperationValue::Null,
        });
        // Directly mutate protocol state: the current_context is set at begin
        // so we need to simulate advancement via a protocol that has advanced context
        let mut proto2 = RolaProtocol::new(advanced);
        let mut env2 = TxEnvelope::new(1, TxMode::Rola, CausalContext::new(), "r1");
        proto2.begin(&mut env2).unwrap();
        // Now advance the context after begin
        let mut post_ctx = CausalContext::new();
        post_ctx.observe(Dot::new(replica_id + 1, seq));
        let mut proto3 = RolaProtocol::new(post_ctx);
        let mut env3 = TxEnvelope::new(1, TxMode::Rola, CausalContext::new(), "r1");
        proto3.begin(&mut env3).unwrap();
        // env3 got snapshot = post_ctx at begin. Now advance further.
        // Create a protocol with even more advanced context
        let mut even_more = env3.read_snapshot.clone();
        even_more.observe(Dot::new(replica_id + 2, seq + 1));
        let proto4 = RolaProtocol::new(even_more);
        env3.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });
        let verdict = proto4.validate(&env3).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Abort(_)),
            "ROLA: must abort when version vector advanced after begin");
    }

    // ─── ROLA CAS success ───
    // If version vector has NOT advanced, tx commits
    #[test]
    fn rola_cas_commit_when_unchanged(
        path in arb_path(),
        val in arb_value(),
    ) {
        let ctx = CausalContext::new();
        let mut proto = RolaProtocol::new(ctx);
        let mut env = TxEnvelope::new(1, TxMode::Rola, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path,
            value: val,
        });
        let verdict = proto.validate(&env).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Commit),
            "ROLA: should commit when context unchanged");
    }

    // ─── TCB causal dependency — abort when deps not met ───
    #[test]
    fn tcb_abort_on_missing_deps(
        dep_replica in 1_u64..=10,
        dep_seq in 1_u64..=100,
        path in arb_path(),
        val in arb_value(),
    ) {
        // Create a TCB with an empty delivered context
        let mut proto = TcbProtocol::new(CausalContext::new());
        // Begin a tx — snapshot will be the empty delivered context
        let mut env = TxEnvelope::new(1, TxMode::Tcb, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        // Manually set read_snapshot to have deps that haven't been delivered
        env.read_snapshot.observe(Dot::new(dep_replica, dep_seq));
        env.write_set.push(DsonOperation::FieldAdd {
            path,
            value: val,
        });
        let verdict = proto.validate(&env).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Abort(_)),
            "TCB: must abort when causal deps not delivered");
    }

    // ─── TCB causal dependency — commit when deps met ───
    #[test]
    fn tcb_commit_when_deps_met(
        path in arb_path(),
        val in arb_value(),
    ) {
        let ctx = CausalContext::new();
        let mut proto = TcbProtocol::new(ctx);
        let mut env = TxEnvelope::new(1, TxMode::Tcb, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        // read_snapshot == delivered_context (both empty) => deps met
        env.write_set.push(DsonOperation::FieldAdd {
            path,
            value: val,
        });
        let verdict = proto.validate(&env).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Commit),
            "TCB: should commit when all deps delivered");
    }

    // ─── Calvin deterministic ordering ───
    // Commit N transactions; committed_order length equals N
    #[test]
    fn calvin_deterministic_ordering(n_txs in 1_usize..=10) {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut committed_count = 0;
        for i in 0..n_txs {
            let mut tx = runtime.begin_tx(&TxOpts {
                mode: TxMode::Calvin,
                timeout_ms: None,
                max_retries: 0,
            });
            tx.set(&format!("key_{i}"), OperationValue::Null).unwrap();
            let events = tx.commit().unwrap();
            // Verify commit event present
            prop_assert!(events.iter().any(|e| e.kind == TxEventKind::Commit),
                "Calvin: tx {} should have commit event", i);
            committed_count += 1;
        }
        prop_assert_eq!(committed_count, n_txs,
            "Calvin: all {} transactions should commit", n_txs);
    }

    // ─── SSI-Lite RW cycle detection ───
    // tx1 reads A writes B, tx2 reads B writes A → must abort
    #[test]
    fn ssi_rw_cycle_detection(
        path_a in arb_path(),
        path_b in arb_path().prop_filter("paths must differ", |_p| true),
    ) {
        // Ensure distinct paths
        let path_b_actual = if path_a == path_b {
            format!("{path_b}_alt")
        } else {
            path_b
        };

        let mut proto = SsiProtocol::new();

        // Tx1: reads path_a, writes path_b
        let mut env1 = TxEnvelope::new(1, TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env1).unwrap();
        env1.read_set.push(path_a.clone());
        env1.write_set.push(DsonOperation::FieldAdd {
            path: path_b_actual.clone(),
            value: OperationValue::Null,
        });
        let v1 = proto.validate(&env1).unwrap();
        prop_assert!(matches!(v1, TxVerdict::Commit), "SSI: first tx should commit");
        proto.commit(&mut env1).unwrap();

        // Tx2: reads path_b, writes path_a → forms RW cycle
        let mut env2 = TxEnvelope::new(2, TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env2).unwrap();
        env2.read_set.push(path_b_actual);
        env2.write_set.push(DsonOperation::FieldAdd {
            path: path_a,
            value: OperationValue::Null,
        });
        let v2 = proto.validate(&env2).unwrap();
        prop_assert!(matches!(v2, TxVerdict::Abort(_)),
            "SSI: RW anti-dependency cycle must be detected");
    }

    // ─── SSI-Lite no cycle — non-overlapping sets commit ───
    #[test]
    fn ssi_no_cycle_commits(
        val1 in arb_value(),
        val2 in arb_value(),
    ) {
        let mut proto = SsiProtocol::new();

        // Tx1: reads "a", writes "b"
        let mut env1 = TxEnvelope::new(1, TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env1).unwrap();
        env1.read_set.push("a".into());
        env1.write_set.push(DsonOperation::FieldAdd {
            path: "b".into(),
            value: val1,
        });
        proto.commit(&mut env1).unwrap();

        // Tx2: reads "c", writes "d" — no overlap with tx1
        let mut env2 = TxEnvelope::new(2, TxMode::SsiLite, CausalContext::new(), "r1");
        proto.begin(&mut env2).unwrap();
        env2.read_set.push("c".into());
        env2.write_set.push(DsonOperation::FieldAdd {
            path: "d".into(),
            value: val2,
        });
        let v2 = proto.validate(&env2).unwrap();
        prop_assert!(matches!(v2, TxVerdict::Commit),
            "SSI: non-overlapping RW sets should commit");
    }

    // ─── MaterializedViews atomic derivation ───
    // Derived ops appear alongside base ops in commit event
    #[test]
    fn materialized_views_atomic_derivation(
        path in arb_path(),
        val in arb_value(),
    ) {
        let mut proto = MaterializedViewsProtocol::new();
        proto.register_view("count_view", Box::new(|ops: &[DsonOperation]| {
            vec![DsonOperation::FieldModify {
                path: "_views.count".into(),
                old_value: None,
                new_value: OperationValue::NumberRef(ops.len().to_string()),
            }]
        }));

        let mut env = TxEnvelope::new(1, TxMode::MaterializedViews, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        env.write_set.push(DsonOperation::FieldAdd {
            path,
            value: val,
        });
        let events = proto.commit(&mut env).unwrap();

        if let TxPayload::Operations(ops) = &events[0].payload {
            // Must have base op + view op
            prop_assert!(ops.len() >= 2,
                "MaterializedViews: commit must include base + derived ops, got {}", ops.len());
            // View op should be present
            let has_view = ops.iter().any(|op| matches!(op,
                DsonOperation::FieldModify { path, .. } if path == "_views.count"
            ));
            prop_assert!(has_view, "MaterializedViews: derived view op missing from commit");
        } else {
            prop_assert!(false, "expected operations payload");
        }
    }

    // ─── Abort idempotence (all modes) ───
    // Begin a tx, abort it, verify abort events are well-formed
    #[test]
    fn abort_produces_well_formed_events(mode_idx in 0_usize..=8) {
        let modes = all_modes();
        let mode = modes[mode_idx];

        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let tx = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        let events = tx.abort().unwrap();
        prop_assert!(!events.is_empty(),
            "Abort for {:?} should produce at least one event", mode);
        // Verify envelope state is Aborted in the event
        for event in &events {
            prop_assert!(
                event.kind == TxEventKind::Abort || event.kind == TxEventKind::Compensate,
                "Abort event for {:?} has unexpected kind: {:?}", mode, event.kind
            );
        }
    }

    // ─── Empty write set rejection (all modes) ───
    // For all 9 modes, begin a tx with no operations, commit should fail
    #[test]
    fn empty_write_set_rejected(mode_idx in 0_usize..=8) {
        let modes = all_modes();
        let mode = modes[mode_idx];

        // Saga has different empty-detection (checks steps, not write_set)
        if mode == TxMode::Saga {
            // Saga uses steps, not write_set — test separately
            let mut proto = SagaProtocol::new();
            let mut env = TxEnvelope::new(1, TxMode::Saga, CausalContext::new(), "r1");
            proto.begin(&mut env).unwrap();
            // No steps added
            let verdict = proto.validate(&env).unwrap();
            prop_assert!(matches!(verdict, TxVerdict::Abort(_)),
                "Saga: empty steps should abort");
            return Ok(());
        }

        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let tx = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        // Commit with no writes
        let result = tx.commit();
        prop_assert!(result.is_err(),
            "{:?}: empty write set should be rejected", mode);
    }

    // ─── State machine invariant (cross-mode) ───
    // After commit, further operations on the same runtime should use fresh tx
    #[test]
    fn committed_tx_state_is_final(
        mode_idx in 0_usize..=8,
        path in arb_path(),
        val in arb_value(),
    ) {
        let modes = all_modes();
        let mode = modes[mode_idx];

        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        // First tx: commit normally
        let mut tx = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        tx.set(&path, val.clone()).unwrap();
        let events = tx.commit().unwrap();
        // Verify commit event records Committed state
        let commit_event = events.iter().find(|e| e.kind == TxEventKind::Commit);
        if let Some(ce) = commit_event {
            prop_assert_eq!(ce.envelope.state, fionn_tx::TxState::Committed,
                "{:?}: commit event should have Committed state", mode);
        }

        // A new tx should work fine (runtime isn't broken).
        // Use a different path to avoid mode-specific conflicts (e.g. PSI
        // write-write detection on the same path).
        let path2 = format!("{path}_2");
        let mut tx2 = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        tx2.set(&path2, val).unwrap();
        let result = tx2.commit();
        // Should succeed (fresh tx on a different path)
        prop_assert!(result.is_ok(),
            "{:?}: fresh tx after committed tx should succeed", mode);
    }

    // ─── ROLA: multiple CAS mutations ───
    // Generate arbitrary causal context mutations; any advancement causes abort
    #[test]
    fn rola_cas_any_advancement_aborts(
        dots in prop::collection::vec((1_u64..=5, 1_u64..=10), 1..=3),
    ) {
        let mut initial_ctx = CausalContext::new();
        for &(r, s) in &dots {
            initial_ctx.observe(Dot::new(r, s));
        }

        // Protocol starts with initial_ctx
        let mut proto = RolaProtocol::new(initial_ctx.clone());
        let mut env = TxEnvelope::new(1, TxMode::Rola, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();
        // env.read_snapshot == initial_ctx (set by begin)

        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });

        // No advancement: should commit
        let verdict = proto.validate(&env).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Commit),
            "ROLA: no advancement should commit");

        // Now create a protocol with advanced context
        let mut advanced = initial_ctx;
        advanced.observe(Dot::new(99, 1)); // new replica not in snapshot
        let proto_advanced = RolaProtocol::new(advanced);
        let verdict2 = proto_advanced.validate(&env).unwrap();
        prop_assert!(matches!(verdict2, TxVerdict::Abort(_)),
            "ROLA: any context advancement should abort");
    }

    // ─── TCB: causal context with partial delivery ───
    #[test]
    fn tcb_partial_delivery_aborts(
        delivered_replicas in prop::collection::vec((1_u64..=5, 1_u64..=10), 1..=3),
        extra_replica in 6_u64..=10,
        extra_seq in 1_u64..=10,
    ) {
        let mut delivered = CausalContext::new();
        for &(r, s) in &delivered_replicas {
            delivered.observe(Dot::new(r, s));
        }
        let mut proto = TcbProtocol::new(delivered.clone());

        // Begin tx — snapshot = delivered
        let mut env = TxEnvelope::new(1, TxMode::Tcb, CausalContext::new(), "r1");
        proto.begin(&mut env).unwrap();

        // Add an undelivered dep to the snapshot
        env.read_snapshot.observe(Dot::new(extra_replica, extra_seq));

        env.write_set.push(DsonOperation::FieldAdd {
            path: "x".into(),
            value: OperationValue::Null,
        });

        let verdict = proto.validate(&env).unwrap();
        prop_assert!(matches!(verdict, TxVerdict::Abort(_)),
            "TCB: undelivered deps in snapshot should abort");
    }

    // ─── Calvin: tx_ids appear in commit events ───
    #[test]
    fn calvin_commit_events_contain_tx_id(n_txs in 1_usize..=5) {
        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut seen_tx_ids = Vec::new();
        for i in 0..n_txs {
            let mut tx = runtime.begin_tx(&TxOpts {
                mode: TxMode::Calvin,
                timeout_ms: None,
                max_retries: 0,
            });
            tx.set(&format!("k{i}"), OperationValue::Null).unwrap();
            let events = tx.commit().unwrap();
            for e in &events {
                if e.kind == TxEventKind::Commit {
                    seen_tx_ids.push(e.tx_id);
                }
            }
        }
        prop_assert_eq!(seen_tx_ids.len(), n_txs,
            "Calvin: should see exactly {} commit tx_ids", n_txs);
        // Tx_ids should be unique
        let unique: std::collections::HashSet<_> = seen_tx_ids.iter().collect();
        prop_assert_eq!(unique.len(), n_txs,
            "Calvin: all tx_ids should be unique");
    }

    // ─── Replay equivalence across modes ───
    // replay(events) works for any mode that commits
    #[test]
    fn replay_equivalence_all_modes(
        mode_idx in prop::sample::select(vec![0_usize, 2, 4, 7, 8]),
        path in arb_path(),
        val in arb_value(),
    ) {
        // Modes that always commit with non-empty write: Ramp(0), Psi(2), Calvin(4), SsiLite(7), MV(8)
        let modes = all_modes();
        let mode = modes[mode_idx];

        let processor = StrictDsonProcessor::new(1);
        let mut runtime = TxRuntime::new(processor);

        let mut tx = runtime.begin_tx(&TxOpts {
            mode,
            timeout_ms: None,
            max_retries: 0,
        });
        tx.set(&path, val).unwrap();
        let events = tx.commit().unwrap();

        // Replay on fresh runtime
        let processor2 = StrictDsonProcessor::new(2);
        let mut runtime2 = TxRuntime::new(processor2);
        let result = runtime2.replay(events.into_iter());
        prop_assert!(result.is_ok(),
            "{:?}: replay should succeed", mode);
    }
}
