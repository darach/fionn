#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Transaction protocol demo — all 9 fionn-tx modes from Python.

Shows how to use the transaction API for:
- Basic commit/abort
- One-shot convenience API
- Multi-replica replication
- Each of the 9 transaction protocol modes

Usage:
    pip install -e .  # or: maturin develop
    python examples/tx_demo.py
"""

from __future__ import annotations

import fionn.ext as fx


def demo_basic_lifecycle() -> None:
    """Basic transaction lifecycle: begin → set → commit."""
    print("=== Basic Transaction Lifecycle ===")

    runtime = fx.TxRuntime(replica_id=1)

    # Begin a transaction
    tx = runtime.begin(fx.TxMode.Ramp)
    print(f"  Transaction state: {tx.state}")

    # Buffer operations
    tx.set("name", "Alice")
    tx.set("age", 30)
    tx.set("active", True)
    print(f"  Buffered {tx.num_operations} operations")

    # Commit
    bundle = runtime.commit_tx(tx)
    print(f"  Committed: {bundle.events[0]}")
    print(f"  Transaction state: {tx.state}")
    print()


def demo_abort() -> None:
    """Transaction abort discards all buffered operations."""
    print("=== Transaction Abort ===")

    runtime = fx.TxRuntime(replica_id=1)

    tx = runtime.begin(fx.TxMode.Ramp)
    tx.set("should_not_persist", "discarded")
    events = runtime.abort_tx(tx)
    print(f"  Aborted: {events[0]}")
    print(f"  Transaction state: {tx.state}")
    print()


def demo_quick_tx() -> None:
    """One-shot convenience API for simple operations."""
    print("=== Quick Transaction (One-Shot) ===")

    events = fx.quick_tx(
        fx.TxMode.Ramp,
        {"user": "Bob", "score": 100, "level": 5},
    )
    print(f"  Quick tx result: {events[0]}")
    print()


def demo_execute() -> None:
    """Execute API: begin + apply + commit in one call."""
    print("=== Execute API ===")

    runtime = fx.TxRuntime(replica_id=1)
    events = runtime.execute(
        fx.TxMode.Calvin,
        {"order_id": "ORD-001", "total": 99.99, "status": "confirmed"},
    )
    print(f"  Executed (Calvin mode): {events[0]}")
    print()


def demo_replication() -> None:
    """Replicate events between two runtimes."""
    print("=== Multi-Replica Replication ===")

    r1 = fx.TxRuntime(replica_id=1)
    r2 = fx.TxRuntime(replica_id=2)

    # Replica 1 commits a transaction
    tx1 = r1.begin(fx.TxMode.Ramp)
    tx1.set("user", "Alice")
    tx1.set("balance", 1000)
    b1 = r1.commit_tx(tx1)
    print(f"  Replica 1 committed: tx_id={b1.events[0].tx_id}")

    # Replica 2 commits a different transaction
    tx2 = r2.begin(fx.TxMode.Ramp)
    tx2.set("user", "Bob")
    tx2.set("balance", 500)
    b2 = r2.commit_tx(tx2)
    print(f"  Replica 2 committed: tx_id={b2.events[0].tx_id}")

    # Cross-deliver events
    r2.apply_events([b1.to_event_data()])
    r1.apply_events([b2.to_event_data()])
    print("  Events exchanged between replicas")
    print()


def demo_all_modes() -> None:
    """Demonstrate all 9 transaction protocol modes."""
    print("=== All 9 Transaction Modes ===")
    print(f"  Available modes: {fx.tx_modes()}")
    print()

    modes = [
        (fx.TxMode.Ramp, "RAMP — Read-Atomic Multi-Partition"),
        (fx.TxMode.Rola, "ROLA — RAMP + Compare-And-Swap"),
        (fx.TxMode.Psi, "PSI — Parallel Snapshot Isolation"),
        (fx.TxMode.Tcb, "TCB — Transactional Causal Broadcast"),
        (fx.TxMode.Calvin, "Calvin — Deterministic Execution"),
        (fx.TxMode.Saga, "Saga — Compensating Transactions"),
        (fx.TxMode.EscrowCounters, "Escrow — Pre-allocated Budgets"),
        (fx.TxMode.SsiLite, "SSI-Lite — Serializable Snapshot Isolation"),
        (fx.TxMode.MaterializedViews, "MatViews — Atomic Derived Views"),
    ]

    for mode, description in modes:
        runtime = fx.TxRuntime(replica_id=1)
        try:
            events = runtime.execute(mode, {"key": "value"})
            status = f"committed ({events[0].num_operations} ops)"
        except ValueError as e:
            status = f"aborted: {e}"
        print(f"  {description}: {status}")

    print()


def demo_sequential_transactions() -> None:
    """Multiple sequential transactions on a single runtime."""
    print("=== Sequential Transactions ===")

    runtime = fx.TxRuntime(replica_id=1)

    for i in range(5):
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set(f"item_{i}", {"id": i, "value": f"data_{i}"})
        bundle = runtime.commit_tx(tx)
        print(f"  Tx {i}: tx_id={bundle.events[0].tx_id}, ops={bundle.events[0].num_operations}")

    print()


if __name__ == "__main__":
    demo_basic_lifecycle()
    demo_abort()
    demo_quick_tx()
    demo_execute()
    demo_replication()
    demo_all_modes()
    demo_sequential_transactions()
    print("All demos completed successfully!")
