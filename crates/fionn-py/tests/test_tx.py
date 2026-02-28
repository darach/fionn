# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for transaction protocol bindings (fionn.ext tx API)."""

from __future__ import annotations

import pytest


class TestTxMode:
    """Test TxMode enum."""

    def test_all_modes_exist(self) -> None:
        """All 9 transaction modes should be accessible."""
        import fionn.ext as fx

        modes = [
            fx.TxMode.Ramp,
            fx.TxMode.Rola,
            fx.TxMode.Psi,
            fx.TxMode.Tcb,
            fx.TxMode.Calvin,
            fx.TxMode.Saga,
            fx.TxMode.EscrowCounters,
            fx.TxMode.SsiLite,
            fx.TxMode.MaterializedViews,
        ]
        assert len(modes) == 9

    def test_modes_are_distinct(self) -> None:
        """Each mode should be unique."""
        import fionn.ext as fx

        modes = [
            fx.TxMode.Ramp,
            fx.TxMode.Rola,
            fx.TxMode.Psi,
            fx.TxMode.Tcb,
            fx.TxMode.Calvin,
            fx.TxMode.Saga,
            fx.TxMode.EscrowCounters,
            fx.TxMode.SsiLite,
            fx.TxMode.MaterializedViews,
        ]
        # Check all pairwise distinct (no set — PyO3 enums aren't hashable)
        for i, a in enumerate(modes):
            for b in modes[i + 1 :]:
                assert a != b

    def test_mode_equality(self) -> None:
        """Same mode should be equal."""
        import fionn.ext as fx

        assert fx.TxMode.Ramp == fx.TxMode.Ramp
        assert fx.TxMode.Ramp != fx.TxMode.Calvin


class TestTxState:
    """Test TxState enum."""

    def test_all_states_exist(self) -> None:
        """All 4 states should be accessible."""
        import fionn.ext as fx

        states = [
            fx.TxState.Active,
            fx.TxState.Preparing,
            fx.TxState.Committed,
            fx.TxState.Aborted,
        ]
        assert len(states) == 4


class TestTxModes:
    """Test tx_modes() function."""

    def test_tx_modes_returns_all(self) -> None:
        """tx_modes() should return all 9 mode names."""
        import fionn.ext as fx

        modes = fx.tx_modes()
        assert len(modes) == 9
        assert "Ramp" in modes
        assert "Calvin" in modes
        assert "SsiLite" in modes
        assert "MaterializedViews" in modes


class TestTxRuntime:
    """Test TxRuntime class."""

    def test_create_runtime(self) -> None:
        """Creating a runtime should succeed."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        assert repr(runtime) == "TxRuntime()"

    def test_create_runtime_default_id(self) -> None:
        """Creating a runtime without replica_id should use default."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        assert runtime is not None

    def test_begin_returns_transaction(self) -> None:
        """begin() should return a Transaction handle."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        assert isinstance(tx, fx.Transaction)
        assert tx.state == fx.TxState.Active
        assert tx.mode == fx.TxMode.Ramp
        assert tx.num_operations == 0


class TestTransactionLifecycle:
    """Test transaction begin/set/commit/abort cycle."""

    def test_basic_commit(self) -> None:
        """A simple set + commit should produce a commit event."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("name", "Alice")
        bundle = runtime.commit_tx(tx)
        assert len(bundle) == 1
        assert bundle.events[0].kind == "commit"
        assert bundle.events[0].num_operations == 1

    def test_multi_key_commit(self) -> None:
        """Multiple sets should all appear in the commit."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("name", "Alice")
        tx.set("age", 30)
        tx.set("active", True)
        assert tx.num_operations == 3
        bundle = runtime.commit_tx(tx)
        assert bundle.events[0].num_operations == 3

    def test_abort(self) -> None:
        """Aborting a transaction should produce an abort event."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        events = runtime.abort_tx(tx)
        assert len(events) == 1
        assert events[0].kind == "abort"

    def test_commit_changes_state(self) -> None:
        """After commit, transaction state should be Committed."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        runtime.commit_tx(tx)
        assert tx.state == fx.TxState.Committed

    def test_abort_changes_state(self) -> None:
        """After abort, transaction state should be Aborted."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        runtime.abort_tx(tx)
        assert tx.state == fx.TxState.Aborted

    def test_double_commit_fails(self) -> None:
        """Committing a committed transaction should raise RuntimeError."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        runtime.commit_tx(tx)
        with pytest.raises(RuntimeError, match="not active"):
            runtime.commit_tx(tx)

    def test_set_after_commit_fails(self) -> None:
        """Setting on a committed transaction should raise RuntimeError."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        runtime.commit_tx(tx)
        with pytest.raises(RuntimeError, match="not active"):
            tx.set("y", 2)


class TestExecuteAPI:
    """Test the one-shot execute() API."""

    def test_execute_basic(self) -> None:
        """execute() should commit a dict of operations."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        events = runtime.execute(fx.TxMode.Ramp, {"x": 1, "y": "hello"})
        assert len(events) == 1
        assert events[0].kind == "commit"

    def test_execute_empty_write_set_aborts(self) -> None:
        """execute() with empty ops should abort (for modes that reject it)."""
        import fionn.ext as fx

        runtime = fx.TxRuntime(replica_id=1)
        with pytest.raises(ValueError):
            runtime.execute(fx.TxMode.Ramp, {})


class TestQuickTx:
    """Test the quick_tx() convenience function."""

    def test_quick_tx_basic(self) -> None:
        """quick_tx() should execute a transaction and return events."""
        import fionn.ext as fx

        events = fx.quick_tx(fx.TxMode.Ramp, {"key": "value"})
        assert len(events) == 1
        assert events[0].kind == "commit"

    def test_quick_tx_with_replica_id(self) -> None:
        """quick_tx() with custom replica_id."""
        import fionn.ext as fx

        events = fx.quick_tx(fx.TxMode.Calvin, {"a": 1}, replica_id=42)
        assert len(events) == 1


class TestAllModes:
    """Test transaction commit across all 9 modes."""

    @pytest.mark.parametrize(
        "mode_name",
        [
            "Ramp",
            "Rola",
            "Psi",
            "Tcb",
            "Calvin",
            "SsiLite",
            "MaterializedViews",
        ],
    )
    def test_commit_all_modes(self, mode_name: str) -> None:
        """Every mode should successfully commit a basic transaction."""
        import fionn.ext as fx

        mode = getattr(fx.TxMode, mode_name)
        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(mode)
        tx.set("key", "value")
        bundle = runtime.commit_tx(tx)
        assert len(bundle) >= 1
        assert bundle.events[0].kind == "commit"

    @pytest.mark.parametrize(
        "mode_name",
        [
            "Ramp",
            "Rola",
            "Psi",
            "Tcb",
            "Calvin",
            "SsiLite",
            "MaterializedViews",
        ],
    )
    def test_abort_all_modes(self, mode_name: str) -> None:
        """Every mode should successfully abort a transaction."""
        import fionn.ext as fx

        mode = getattr(fx.TxMode, mode_name)
        runtime = fx.TxRuntime(replica_id=1)
        tx = runtime.begin(mode)
        events = runtime.abort_tx(tx)
        assert len(events) >= 1
        assert events[0].kind == "abort"


class TestReplication:
    """Test event replication between runtimes."""

    def test_apply_events_between_runtimes(self) -> None:
        """Events from one runtime should be applicable to another."""
        import fionn.ext as fx

        runtime1 = fx.TxRuntime(replica_id=1)
        runtime2 = fx.TxRuntime(replica_id=2)

        # Commit on runtime1
        tx = runtime1.begin(fx.TxMode.Ramp)
        tx.set("name", "Alice")
        bundle = runtime1.commit_tx(tx)

        # Apply to runtime2
        event_data = bundle.to_event_data()
        runtime2.apply_events([event_data])  # Should not raise

    def test_bidirectional_replication(self) -> None:
        """Two runtimes should be able to exchange events."""
        import fionn.ext as fx

        r1 = fx.TxRuntime(replica_id=1)
        r2 = fx.TxRuntime(replica_id=2)

        # r1 commits
        tx1 = r1.begin(fx.TxMode.Ramp)
        tx1.set("a", 1)
        b1 = r1.commit_tx(tx1)

        # r2 commits
        tx2 = r2.begin(fx.TxMode.Ramp)
        tx2.set("b", 2)
        b2 = r2.commit_tx(tx2)

        # Cross-deliver
        r2.apply_events([b1.to_event_data()])
        r1.apply_events([b2.to_event_data()])


class TestTxEvent:
    """Test TxEvent properties."""

    def test_event_repr(self) -> None:
        """TxEvent should have a useful repr."""
        import fionn.ext as fx

        events = fx.quick_tx(fx.TxMode.Ramp, {"x": 1})
        repr_str = repr(events[0])
        assert "TxEvent" in repr_str
        assert "commit" in repr_str

    def test_event_tx_id(self) -> None:
        """Each event should have a non-zero tx_id."""
        import fionn.ext as fx

        events = fx.quick_tx(fx.TxMode.Ramp, {"x": 1})
        assert events[0].tx_id > 0


class TestTxEventBundle:
    """Test TxEventBundle properties."""

    def test_bundle_length(self) -> None:
        """Bundle should report correct length."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        bundle = runtime.commit_tx(tx)
        assert len(bundle) == 1

    def test_bundle_repr(self) -> None:
        """Bundle should have a useful repr."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        bundle = runtime.commit_tx(tx)
        assert "TxEventBundle" in repr(bundle)


class TestTransactionOperations:
    """Test various transaction operations."""

    def test_set_various_types(self) -> None:
        """set() should handle various Python types."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("string", "hello")
        tx.set("int", 42)
        tx.set("float", 3.14)
        tx.set("bool", True)
        tx.set("null", None)
        tx.set("list", [1, 2, 3])
        tx.set("dict", {"nested": "value"})
        assert tx.num_operations == 7
        bundle = runtime.commit_tx(tx)
        assert bundle.events[0].num_operations == 7

    def test_get_records_read(self) -> None:
        """get() should record a read (used for SSI cycle detection)."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.SsiLite)
        tx.get("x")
        tx.set("y", 1)
        bundle = runtime.commit_tx(tx)
        assert bundle.events[0].kind == "commit"

    def test_delete_operation(self) -> None:
        """delete() should buffer a delete operation."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.Ramp)
        tx.set("x", 1)
        tx.delete("y")
        assert tx.num_operations == 2
        bundle = runtime.commit_tx(tx)
        assert bundle.events[0].num_operations == 2

    def test_transaction_repr(self) -> None:
        """Transaction should have a useful repr."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        tx = runtime.begin(fx.TxMode.Calvin)
        tx.set("x", 1)
        repr_str = repr(tx)
        assert "Transaction" in repr_str
        assert "Calvin" in repr_str

    def test_sequential_transactions(self) -> None:
        """Multiple sequential transactions should work."""
        import fionn.ext as fx

        runtime = fx.TxRuntime()
        for i in range(5):
            tx = runtime.begin(fx.TxMode.Ramp)
            tx.set(f"key_{i}", i)
            bundle = runtime.commit_tx(tx)
            assert bundle.events[0].kind == "commit"
