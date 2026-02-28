# fionn-tx

[![Crates.io](https://img.shields.io/crates/v/fionn-tx.svg)](https://crates.io/crates/fionn-tx)
[![Documentation](https://docs.rs/fionn-tx/badge.svg)](https://docs.rs/fionn-tx)
[![License](https://img.shields.io/crates/l/fionn-tx.svg)](https://github.com/darach/fionn#license)

Transactional envelopes for fionn CRDTs.

CRDTs guarantee convergence, but many applications need stronger guarantees around
groups of operations -- read-atomic visibility, snapshot isolation, causal ordering,
or compensating rollback. `fionn-tx` provides nine transaction protocols over Strict
DSON, each suited to a different consistency-availability tradeoff.

A `TxRuntime` manages the underlying `StrictDsonProcessor` and exposes a
`begin_tx` / `commit` / `abort` API. Operations within a transaction are buffered,
validated against the protocol's rules, and applied atomically on commit.

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the underlying CRDT primitives, see
[fionn-crdt](https://crates.io/crates/fionn-crdt).

## Transaction Modes

### RAMP — Read-Atomic Multi-Partition

Stamps every write with the transaction id. A reader that sees any write from a
transaction fetches all of that transaction's writes before returning. No two-phase
commit. No coordination. Readers pay the cost of completeness; writers pay nothing
beyond the stamp.

**Use case: Butterfly swap settlement**

A butterfly spread on crude oil futures settles as three legs across three clearing
partitions: buy the low-strike call, sell two at-the-money calls (1:2:1 ratio), buy
the high-strike call. Three legs, three strikes, four contracts. The clearinghouse
writes each leg to its partition's JSON ledger. A risk auditor querying the ledger must
see all three legs or none — a partial view would show a naked short that never existed.
RAMP's read-atomic guarantee prevents this phantom exposure without locking any
partition.

Fionn's gron flattens each leg to `settlement.legs[n].strike` paths for bulk
reconciliation. The skip tape scans only `settlement.status` across millions of
contracts without parsing nested instrument metadata. `GenericPatch` computes the
minimal ledger mutation; the RAMP envelope wraps the three-leg batch.

```
cargo bench --bench tx_scenarios -- ramp
```

### ROLA — RAMP + Compare-And-Swap

Extends RAMP with a version vector check at commit time. If the causal context has
advanced since the transaction began, the commit fails. The caller retries with fresh
state.

**Use case: OTA firmware update with immutable swap and recovery**

An avionics supplier pushes a firmware update to a fleet of engine control units. Each
ECU holds two config slots — active and standby — as JSON documents. The update writes
the new firmware image descriptor to the standby slot, then atomically swaps
`config.active_slot` from `"A"` to `"B"`. If a field technician patched a sensor
calibration table on the active slot between the supplier's read and the swap, the CAS
fails. The ECU stays on slot A. The supplier sees the conflict, merges the calibration
patch into the new image, and retries. If the swap succeeds but the ECU fails POST,
recovery reads the prior active slot from the commit event log and reverts the pointer.

The `CausalContext` serves as the version vector — no separate OT layer. Schema
filtering targets only `config.active_slot` and `config.standby.*`, so unrelated
telemetry writes do not trigger false CAS failures. Tape serialization produces
zero-copy config snapshots for transmission over constrained CAN-FD links.

```
cargo bench --bench tx_scenarios -- rola
```

### PSI — Parallel Snapshot Isolation

Transactions read from a consistent snapshot. Writes to the same path by concurrent
transactions are detected at validation time. Exactly one wins. The other aborts and
retries.

**Use case: CDS/CDR contract repricing under basis-differential billing**

An energy trading desk prices Credit Default Swap contracts against a shared reference
document that holds basis curves, counterparty exposures, and CDR (Constant Default
Rate) assumptions. Two pricing engines run concurrently: one reprices the IG (investment
grade) tranche using updated CDR curves, the other reprices the HY (high yield) tranche
using updated recovery rates. Both tranches share the `portfolio.total_exposure` field.
If both engines write to `portfolio.total_exposure` in the same billing cycle, PSI
detects the write-write conflict and aborts one — preventing a double-count that would
misstate the desk's net exposure to the risk committee.

JSONL streaming processes the CDR tick feed through the skip tape without buffering.
`OptimizedMergeProcessor` applies the `Additive` strategy for exposure aggregation and
`Max` for timestamps. `PreParsedValue::Integer` keeps basis-point deltas in `i64`
through the merge pipeline with no parse-format round trip per tick.

```
cargo bench --bench tx_scenarios -- psi
```

### TCB — Transactional Causal Broadcast

Buffers operations until all causal dependencies are delivered, then applies them
atomically. Prevents the "effect before cause" anomaly across replicas.

**Use case: Cross-region MMO inventory transfer**

A fantasy MMO runs world servers in three regions. A guild leader in EU-West trades a
legendary sword to a player in US-East. That player immediately enchants the sword and
mails it to a friend in AP-Southeast. Every region must see the original trade before
the enchantment, and the enchantment before the mail. TCB buffers each step until its
causal predecessor is delivered — no region ever sees a sword enchanted before it was
received, or mailed before it was enchanted.

`StrictDsonProcessor`'s dot-based causal context encodes the happened-before chain.
Delta generation sends only changed inventory slots cross-region, not full inventories.
`DotObservedRemoveMap`'s add-wins semantics ensure that concurrent loot pickups from
the same chest are both preserved for game logic to adjudicate.

```
cargo bench --bench tx_scenarios -- tcb
```

### Calvin — Deterministic Execution

Transactions agree on execution order first, then each replica executes the agreed
sequence without coordination. Identical input produces identical output on every node.

**Use case: Deterministic LLM eval pipeline reproduction**

An AI lab benchmarks a model across 64 GPU nodes. Each node scores a shard of prompts
and writes results to a shared JSON document. For publication, every node must produce
byte-identical output from the same seed. Calvin's sequencer assigns a deterministic
execution order. Each node executes its shard in that order. The committed event stream
is archived alongside model weights as the reproducibility provenance record.

Tape-to-tape transforms run scoring functions without materializing to
`serde_json::Value`. `CanonicalOperationProcessor` ensures deterministic key ordering
in the results document. The `TxEvent` stream is the audit log — replay it on any node
for identical results.

```
cargo bench --bench tx_scenarios -- calvin
```

### Saga — Compensating Transactions

A sequence of steps, each with a compensating action. If step N fails, steps 1 through
N-1 are compensated in reverse order.

**Use case: KYC document verification pipeline**

A bank onboards a new customer. The pipeline: parse the JSON application, validate
against regulatory schema, enrich with credit bureau data, convert to the bank's
canonical TOML format, write to the compliance store. If the credit bureau returns a
sanctions hit at step three, the partially enriched application must be purged from all
intermediate stores. Each step's compensation is the reverse patch.

Multi-format support handles JSON to schema validation to TOML natively.
`GenericPatch` records each forward step's diff; the compensation applies the reverse
via `Patchable`. `CompiledSchema` enforces the regulatory schema, with `SchemaFilter`
limiting which fields pass to the compliance store.

```
cargo bench --bench tx_scenarios -- saga
```

### Escrow Counters — Bounded Distributed Counters

Each replica holds a pre-allocated budget. Local decrements within the budget need no
coordination. The global floor invariant — the counter never drops below zero — is
maintained regardless of message delays.

**Use case: Prepaid data quota enforcement across cell towers**

A prepaid subscriber has 5 GB remaining. Two cell towers serve the subscriber
concurrently during a handover. Without coordination, both towers could decrement
past zero. Each tower gets an escrow allocation of 500 MB. Tower A decrements locally
at line rate. When its budget is exhausted, it requests more from the OCS. The quota
never goes negative, even if the OCS is temporarily unreachable.

JSONL usage events arrive at 100k/sec per tower. `fionn-stream` processes each event
through the skip tape with the escrow check per line. `OperationValue::NumberRef`
stores the quota as string-precision to avoid floating-point drift in billing.
`fionn-pool` reuses tape buffers across events; budget exhaustion triggers
backpressure before the pool is saturated.

```
cargo bench --bench tx_scenarios -- escrow
```

### SSI-Lite — Serializable Snapshot Isolation

Extends snapshot isolation with read-write anti-dependency tracking. If a dangerous
structure is detected — transaction A reads what B wrote, and B reads what A wrote —
one transaction is aborted. The committed history is serializable.

**Use case: Portfolio risk snapshot consistency**

A risk engine reads `portfolio.positions` and `market.prices` to compute Value-at-Risk.
A rebalancing engine concurrently writes new positions and its own price snapshot. If
the risk engine reads stale positions with fresh prices, VaR is computed from a state
that never existed — a phantom number that could trigger an erroneous margin call. SSI
detects the RW cycle and aborts one transaction.

Path-based read and write sets track `portfolio.positions.*` and `market.prices.*`
directly using fionn's `ParsedPath`. The skip tape reads only those two subtrees,
skipping hundreds of metadata fields. The write set is derived from diff computation,
so RW tracking comes for free.

```
cargo bench --bench tx_scenarios -- ssi
```

### Materialized Views — Atomic Base + View Updates

Mutations to a base document and its derived views are applied in a single transaction.
No query ever sees a stale view.

**Use case: Live gron index for prompt template search**

A prompt engineering platform stores 50k templates as nested JSON. Users need
sub-millisecond grep across all templates by any field. The platform maintains a
secondary gron index — a flattened `path=value` representation. Every mutation atomically
updates both the base document and the gron index. A search never returns a deleted
template, and never misses a newly created one.

`fionn-gron` is the view derivation function — `gron::encode` on the changed paths.
Tape-level diffing re-grons only changed paths, not entire templates. `CompiledSchema`
defines which fields are indexed, keeping the view selective.

```
cargo bench --bench tx_scenarios -- materialized_views
```

## Benchmark Summary

All benchmarks run single-threaded on the transaction protocol logic — no I/O, no
network, no disk. The numbers measure what you pay for correctness guarantees alone.

| Mode | Parameter | Time | Throughput |
|------|-----------|------|------------|
| RAMP | 1 butterfly (3 legs) | 2.0 µs | 1.48 Mleg/s |
| RAMP | 4 butterflies (12 legs) | 12.7 µs | 942 Kleg/s |
| RAMP | 12 butterflies (36 legs) | 35.6 µs | 1.01 Mleg/s |
| ROLA | 10 ECUs | 5.5 µs | 1.83 MECU/s |
| ROLA | 100 ECUs | 59.2 µs | 1.69 MECU/s |
| ROLA | 500 ECUs | 298 µs | 1.68 MECU/s |
| PSI | 2 tranches | 660 ns | 3.03 Mtranche/s |
| PSI | 8 tranches | 1.95 µs | 4.11 Mtranche/s |
| TCB | 3-hop chain | 1.88 µs | 1.59 Mhop/s |
| TCB | 10-hop chain | 11.8 µs | 851 Khop/s |
| Calvin | 100 prompts | 44.3 µs | 2.26 Mprompt/s |
| Calvin | 1000 prompts | 441 µs | 2.27 Mprompt/s |
| Saga | fail at step 0 | 232 ns | — |
| Saga | success (5 steps) | 312 ns | — |
| Escrow | 1k events | 207 µs | 4.83 Mevent/s |
| Escrow | 50k events | 5.0 ms | 9.99 Mevent/s |
| SSI | 2 concurrent txs | 604 ns | 3.31 Mtx/s |
| SSI | 8 concurrent txs | 2.56 µs | 3.12 Mtx/s |
| Mat. Views | 10 templates | 5.8 µs | 1.71 Mtemplate/s |
| Mat. Views | 500 templates | 291 µs | 1.71 Mtemplate/s |

### Reproducing

Run all scenarios:

```
cargo bench --bench tx_scenarios
```

Run a single mode:

```
cargo bench --bench tx_scenarios -- ramp
cargo bench --bench tx_scenarios -- escrow
```

Results land in `target/criterion/tx_scenarios/` with HTML reports.

## Architecture

```
TxRuntime
 ├── StrictDsonProcessor (CRDT state)
 ├── TxProtocol implementations (one per mode)
 └── TxEvent stream (commit log)
```

A `TxRuntime` wraps a `StrictDsonProcessor` and holds one protocol instance per mode.
`begin_tx` returns a `TxHandle` that buffers reads and writes. On `commit`, the
protocol validates (conflict detection, CAS check, escrow bound, etc.) and either
emits `TxEvent`s or aborts. Events are the unit of replication — `apply_events` on a
remote replica replays them.

## References

- Rinberg, A., Upadhyaya, A., & Bailis, P. (2022). DSON: JSON CRDT Using Delta-Mutations For Document Stores. *Proceedings of the VLDB Endowment*, 15(5).
- Bailis, P., Fekete, A., Ghodsi, A., Hellerstein, J. M., & Stoica, I. (2014). Scalable Atomic Visibility with RAMP Transactions. *SIGMOD*.
- Thomson, A., Diamond, T., Weng, S., Ren, K., Shao, P., & Abadi, D. (2012). Calvin: Fast Distributed Transactions for Partitioned Database Systems. *SIGMOD*.
- Cahill, M. J., Röhm, U., & Fekete, A. D. (2008). Serializable Isolation for Snapshot Databases. *SIGMOD*.

## License

MIT OR Apache-2.0
