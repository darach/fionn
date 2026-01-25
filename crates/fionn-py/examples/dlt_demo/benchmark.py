import json
from pathlib import Path

from data_gen import main as generate_data
from pipeline import (
    get_memory_usage,
    run_baseline,
    run_conflict_storm,
    run_conflict_storm_native,
    run_crdt_merge,
    run_csv_to_isonl_ingest,
    run_filtering_baseline,
    run_filtering_fionn,
    run_gron_flattening,
    run_memory_pressure_baseline,
    run_memory_pressure_fionn,
    run_native_jsonl,
    run_orjson,
    run_parallel_crdt_merge,
    run_pooling_baseline,
    run_pooling_fionn,
    run_python_conflict_storm,
    run_python_flattening,
    run_python_merge,
    run_rust_cli_gron,
    run_rust_cli_merge,
    run_rust_cli_merge_stream,
    # Pure Rust CLI (no Python overhead)
    run_rust_cli_stream,
    run_rust_cli_stream_selective,
    run_selective_isonl,
    run_selective_jsonl,
    run_simdjson,
    run_ujson,
    run_ultra,
)


def main():
    results_file = Path("results.json")
    data_dir = Path("data")

    # 1. Generate Wide Data (more realistic for enterprise/dimensional loads)
    num_records = 50000
    wide_jsonl = data_dir / "events_wide.jsonl"
    wide_isonl = data_dir / "events_wide.isonl"
    csv_file = data_dir / "events.csv"

    if not wide_jsonl.exists():
        generate_data(num_records, wide=True)  # Generate wide records

    # 2. Run Benchmarks
    print("\n--- Starting Dimensional Benchmarks ---\n")

    results = {"num_records": num_records, "runs": []}

    selective_fields = ["event_id", "user_id", "score"]

    modes = [
        # Standard Ingest (Full Parse) - stdlib
        ("Baseline (Full JSONL)", run_baseline, str(wide_jsonl), None),
        # Alternative JSON parsers (C/Rust bindings)
        ("orjson (Full JSONL)", run_orjson, str(wide_jsonl), None),
        ("ujson (Full JSONL)", run_ujson, str(wide_jsonl), None),
        ("simdjson (Full JSONL)", run_simdjson, str(wide_jsonl), None),
        # fionn-py (Rust via PyO3) - has DLT pipeline overhead
        ("fionn-py (Full JSONL)", run_native_jsonl, str(wide_jsonl), None),
        # Selective Ingest (Schema-aware Parse)
        ("fionn-py Selective (3 fields)", run_selective_jsonl, str(wide_jsonl), selective_fields),
        ("fionn-py ISONL (3 fields)", run_selective_isonl, str(wide_isonl), selective_fields),
        # Format Agility (Tape-like translation)
        ("fionn-py CSV->ISONL", run_csv_to_isonl_ingest, str(csv_file), None),
        # Ultra Ingest (Optimized Line format)
        ("fionn-py ISONL SIMD", run_ultra, str(wide_isonl), None),
        # ========== PURE RUST (no Python, no DLT overhead) ==========
        ("fionn (Rust) stream", run_rust_cli_stream, str(wide_jsonl), None),
        (
            "fionn (Rust) selective",
            run_rust_cli_stream_selective,
            str(wide_jsonl),
            selective_fields,
        ),
    ]

    import gc

    for name, func, path, args in modes:
        if not Path(path).exists():
            print(f"Skipping {name}, path {path} does not exist.")
            continue

        print(f"Benchmarking {name}...")

        # Force GC and measure memory before
        gc.collect()
        mem_before = get_memory_usage()

        # Actual run
        if args:
            duration, info = func(path, args)
        else:
            duration, info = func(path)

        # Skip if benchmark failed
        if duration is None:
            print(f"  SKIPPED: {info}")
            continue

        # Force GC and measure memory after
        gc.collect()
        mem_after = get_memory_usage()
        mem_delta = max(0, mem_after - mem_before)

        results["runs"].append(
            {
                "name": name,
                "duration": duration,
                "throughput": num_records / duration if duration > 0 else 0,
                "memory_mb": mem_delta,
            }
        )

    # 3. Running Semantic Benchmarks (Advanced Dimensions)
    print("\n--- Starting Semantic Benchmarks (GRON & CRDT) ---\n")

    semantic_modes = [
        # GRON vs Recursive Python
        ("Baseline (Python Flatten)", run_python_flattening, data_dir / "events_nested.jsonl"),
        ("Semantic (Fionn GRON)", run_gron_flattening, data_dir / "events_nested.jsonl"),
        ("fionn (Rust) gron", run_rust_cli_gron, data_dir / "events_nested.jsonl"),
        # CRDT vs Manual Dict Update
        (
            "Baseline (Python Merge)",
            run_python_merge,
            (data_dir / "profiles_a.jsonl", data_dir / "profiles_b.jsonl"),
        ),
        (
            "Semantic (Fionn CRDT)",
            run_crdt_merge,
            (data_dir / "profiles_a.jsonl", data_dir / "profiles_b.jsonl"),
        ),
        (
            "Semantic (Parallel CRDT)",
            run_parallel_crdt_merge,
            (data_dir / "profiles_a.jsonl", data_dir / "profiles_b.jsonl"),
        ),
        (
            "fionn (Rust) merge",
            run_rust_cli_merge,
            (data_dir / "profiles_a.jsonl", data_dir / "profiles_b.jsonl"),
        ),
        # Conflict Storm (Multi-core scaling)
        ("Baseline (Python Storm)", run_python_conflict_storm, (20000, 4)),
        ("Semantic (Fionn Storm)", run_conflict_storm, (20000, 4)),
        ("Semantic (Native Storm)", run_conflict_storm_native, data_dir / "storm_updates.jsonl"),
        ("fionn (Rust) stream stats", run_rust_cli_merge_stream, data_dir / "storm_updates.jsonl"),
        # Filtering (Row Skipping)
        (
            "Baseline (Python Filter)",
            run_filtering_baseline,
            (data_dir / "events_wide.jsonl", "event_type", "purchase"),
        ),
        (
            "Semantic (Fionn Filter)",
            run_filtering_fionn,
            (data_dir / "events_wide.jsonl", "event_type", "purchase"),
        ),
        # Memory Pressure (DOM vs Streaming)
        ("Baseline (DOM Memory)", run_memory_pressure_baseline, data_dir / "events_wide.jsonl"),
        ("Semantic (Fionn Stream)", run_memory_pressure_fionn, data_dir / "events_wide.jsonl"),
        # Native Pooling (Allocation Churn)
        ("Baseline (No Pooling)", run_pooling_baseline, 100000),
        ("Semantic (Tape Pooling)", run_pooling_fionn, 100000),
    ]

    for name, func, paths in semantic_modes:
        print(f"Benchmarking {name}...")

        # Force GC and measure memory before
        gc.collect()
        mem_before = get_memory_usage()

        if isinstance(paths, tuple):
            # Only convert Path objects to strings, keep others (like int) as is
            args = [str(p) if isinstance(p, Path) else p for p in paths]
            duration, info = func(*args)
        else:
            arg = str(paths) if isinstance(paths, Path) else paths
            duration, info = func(arg)

        # Skip if benchmark failed
        if duration is None:
            print(f"  SKIPPED: {info}")
            continue

        # Force GC and measure memory after
        gc.collect()
        mem_after = get_memory_usage()
        mem_delta = max(0, mem_after - mem_before)

        results["runs"].append(
            {
                "name": name,
                "duration": duration,
                "throughput": 1 / duration if duration > 0 else 0,
                "memory_mb": mem_delta,
            }
        )

    # Save results
    with open(results_file, "w") as f:
        json.dump(results, f, indent=2)

    print(f"\nResults saved to {results_file}")

    # Summary
    print("\n--- Summary ---")
    baseline = results["runs"][0]["duration"]
    for run in results["runs"]:
        speedup = baseline / run["duration"]
        mem = run.get("memory_mb", 0)
        print(f"{run['name']}: {run['duration']:.2f}s ({speedup:.2f}x speedup, {mem:.1f} MB RSS)")


if __name__ == "__main__":
    main()
