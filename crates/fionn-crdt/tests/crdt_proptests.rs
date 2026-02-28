// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for CRDT types (Strict DSON mode)

use fionn_crdt::DotObservedRemoveMap;
use fionn_crdt::dot_store::{CausalContext, Dot, DotStore, VecDotStore};
use fionn_crdt::lattice::Lattice;
use fionn_crdt::rga::Rga;
use fionn_crdt::strict::StrictDsonProcessor;

use fionn_core::OperationValue;
use proptest::prelude::*;

// ─── Strategies ────────────────────────────────────────────────────────────

fn arb_replica_id() -> impl Strategy<Value = u64> {
    1_u64..=8
}

fn arb_sequence() -> impl Strategy<Value = u64> {
    1_u64..=100
}

fn arb_dot() -> impl Strategy<Value = Dot> {
    (arb_replica_id(), arb_sequence()).prop_map(|(r, s)| Dot::new(r, s))
}

fn arb_causal_context() -> impl Strategy<Value = CausalContext> {
    prop::collection::vec(arb_dot(), 1..=4).prop_map(|dots| {
        let mut ctx = CausalContext::new();
        for dot in dots {
            ctx.observe(dot);
        }
        ctx
    })
}

fn arb_vec_dot_store() -> impl Strategy<Value = VecDotStore> {
    prop::collection::vec(arb_dot(), 0..=6).prop_map(|dots| {
        let mut store = VecDotStore::new();
        for dot in dots {
            store.add_dot(dot);
        }
        store
    })
}

fn arb_value() -> impl Strategy<Value = OperationValue> {
    prop_oneof![
        "[a-z]{1,5}".prop_map(OperationValue::StringRef),
        any::<i32>().prop_map(|n| OperationValue::NumberRef(n.to_string())),
        any::<bool>().prop_map(OperationValue::BoolRef),
        Just(OperationValue::Null),
    ]
}

fn arb_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b".to_string()),
        Just("c".to_string()),
        Just("x.y".to_string()),
    ]
}

// ─── Lattice laws ──────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    // ─── CausalContext ───

    #[test]
    fn causal_context_join_commutative(a in arb_causal_context(), b in arb_causal_context()) {
        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);
        // Both should observe the same dots
        for r in 1_u64..=8 {
            for s in 1_u64..=100 {
                let dot = Dot::new(r, s);
                prop_assert_eq!(ab.has_observed(dot), ba.has_observed(dot));
            }
        }
    }

    #[test]
    fn causal_context_join_idempotent(a in arb_causal_context()) {
        let before = a.clone();
        let mut after = a;
        after.join(&before);
        for r in 1_u64..=8 {
            for s in 1_u64..=100 {
                let dot = Dot::new(r, s);
                prop_assert_eq!(before.has_observed(dot), after.has_observed(dot));
            }
        }
    }

    #[test]
    fn causal_context_join_associative(
        a in arb_causal_context(),
        b in arb_causal_context(),
        c in arb_causal_context()
    ) {
        // (a ∨ b) ∨ c
        let mut ab_c = a.clone();
        ab_c.join(&b);
        ab_c.join(&c);

        // a ∨ (b ∨ c)
        let mut bc = b.clone();
        bc.join(&c);
        let mut a_bc = a;
        a_bc.join(&bc);

        for r in 1_u64..=8 {
            for s in 1_u64..=100 {
                let dot = Dot::new(r, s);
                prop_assert_eq!(ab_c.has_observed(dot), a_bc.has_observed(dot));
            }
        }
    }

    // ─── VecDotStore ───

    #[test]
    fn vec_dot_store_join_commutative(a in arb_vec_dot_store(), b in arb_vec_dot_store()) {
        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);
        let mut ab_dots = ab.dots();
        let mut ba_dots = ba.dots();
        ab_dots.sort_by_key(|d| (d.replica_id, d.sequence));
        ba_dots.sort_by_key(|d| (d.replica_id, d.sequence));
        prop_assert_eq!(ab_dots, ba_dots);
    }

    #[test]
    fn vec_dot_store_join_idempotent(a in arb_vec_dot_store()) {
        let before = a.clone();
        let mut after = a;
        after.join(&before);
        let mut before_dots = before.dots();
        let mut after_dots = after.dots();
        before_dots.sort_by_key(|d| (d.replica_id, d.sequence));
        after_dots.sort_by_key(|d| (d.replica_id, d.sequence));
        prop_assert_eq!(before_dots, after_dots);
    }

    // ─── RGA convergence ───

    #[test]
    fn rga_merge_commutative(
        vals1 in prop::collection::vec(arb_value(), 0..=4),
        vals2 in prop::collection::vec(arb_value(), 0..=4),
    ) {
        let mut r1 = Rga::new(1);
        for v in &vals1 {
            r1.insert_after(None, v.clone());
        }
        let mut r2 = Rga::new(2);
        for v in &vals2 {
            r2.insert_after(None, v.clone());
        }

        let mut ab = r1.clone();
        ab.merge(&r2);
        let mut ba = r2.clone();
        ba.merge(&r1);

        let ab_vals: Vec<_> = ab.to_vec().iter().map(|v| format!("{v:?}")).collect();
        let ba_vals: Vec<_> = ba.to_vec().iter().map(|v| format!("{v:?}")).collect();
        prop_assert_eq!(ab_vals, ba_vals);
    }

    #[test]
    fn rga_merge_idempotent(vals in prop::collection::vec(arb_value(), 0..=4)) {
        let mut rga = Rga::new(1);
        for v in &vals {
            rga.insert_after(None, v.clone());
        }
        let before = rga.clone();
        rga.merge(&before);
        prop_assert_eq!(rga.len(), before.len());
    }

    // ─── DotObservedRemoveMap ───

    // DotObservedRemoveMap commutativity: use disjoint replica IDs to ensure
    // no dot collisions (which would violate the OR-set invariant that each
    // dot uniquely identifies a single value).
    #[test]
    fn dorm_join_commutative(
        ops_a in prop::collection::vec((arb_path(), arb_value()), 1..=5),
        ops_b in prop::collection::vec((arb_path(), arb_value()), 1..=5),
    ) {
        let mut a = DotObservedRemoveMap::new();
        for (i, (path, val)) in ops_a.iter().enumerate() {
            a.add(path, val.clone(), Dot::new(1, i as u64 + 1));
        }
        let mut b = DotObservedRemoveMap::new();
        for (i, (path, val)) in ops_b.iter().enumerate() {
            b.add(path, val.clone(), Dot::new(2, i as u64 + 1));
        }

        let mut ab = a.clone();
        ab.join(&b);
        let mut ba = b.clone();
        ba.join(&a);

        // Both should have same paths and same number of values per path
        for path in ["a", "b", "c", "x.y"] {
            prop_assert_eq!(ab.get(path).len(), ba.get(path).len(),
                "path {}: ab has {} values, ba has {}", path, ab.get(path).len(), ba.get(path).len());
        }
    }

    #[test]
    fn dorm_join_idempotent(
        ops in prop::collection::vec((arb_path(), arb_value()), 1..=5),
    ) {
        let mut map = DotObservedRemoveMap::new();
        for (i, (path, val)) in ops.iter().enumerate() {
            map.add(path, val.clone(), Dot::new(1, i as u64 + 1));
        }
        let before = map.clone();
        map.join(&before);
        for path in ["a", "b", "c", "x.y"] {
            prop_assert_eq!(map.get(path).len(), before.get(path).len());
        }
    }

    // ─── OR semantics: concurrent add+delete preserves the add ───

    #[test]
    fn or_add_wins_under_concurrency(
        path in arb_path(),
        val in arb_value(),
        initial_dot in arb_dot(),
    ) {
        // Both replicas see an initial add
        let add_dot = Dot::new(initial_dot.replica_id, initial_dot.sequence + 1);

        let mut r1 = DotObservedRemoveMap::new();
        r1.add(&path, OperationValue::Null, initial_dot);

        let mut r2 = r1.clone();

        // r1 adds a new value concurrently
        r1.add(&path, val, add_dot);

        // r2 removes (only observes initial_dot)
        r2.remove(&path);

        // Merge: r1's new add should survive
        r1.join(&r2);
        prop_assert!(r1.contains(&path),
            "add-wins violated: path {} should still exist after concurrent add+delete", path);
    }

    // ─── StrictDsonProcessor convergence ───

    #[test]
    fn strict_processor_convergence(
        ops_a in prop::collection::vec((arb_path(), arb_value()), 1..=5),
        ops_b in prop::collection::vec((arb_path(), arb_value()), 1..=5),
    ) {
        let mut r1 = StrictDsonProcessor::new(1);
        let mut r2 = StrictDsonProcessor::new(2);

        for (path, val) in &ops_a {
            r1.set(path, val.clone());
        }
        for (path, val) in &ops_b {
            r2.set(path, val.clone());
        }

        let mut m1 = r1.clone();
        m1.join(&r2);
        let mut m2 = r2.clone();
        m2.join(&r1);

        prop_assert_eq!(m1.to_json(), m2.to_json(),
            "convergence violated: join(r1,r2) != join(r2,r1)");
    }

    // ─── Delta completeness ───

    #[test]
    fn delta_completeness(
        ops in prop::collection::vec((arb_path(), arb_value()), 1..=5),
    ) {
        let mut source = StrictDsonProcessor::new(1);
        let empty_ctx = fionn_crdt::dot_store::CausalContext::new();

        for (path, val) in &ops {
            source.set(path, val.clone());
        }

        let delta = source.generate_delta(&empty_ctx);

        let mut target = StrictDsonProcessor::new(2);
        target.apply_delta(&delta).unwrap();

        // After applying full delta, target should have same values
        prop_assert_eq!(source.to_json(), target.to_json(),
            "delta completeness violated");
    }
}
