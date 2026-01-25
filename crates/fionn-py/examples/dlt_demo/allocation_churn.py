# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Benchmark for measuring allocation churn and pooling benefits.
This script compares parsing many small JSON objects with and without pooling.
"""

import gc
import os
import time

import fionn.ext as fx
import psutil


def get_memory_usage():
    """Get current RSS memory usage in MB."""
    process = psutil.Process(os.getpid())
    return process.memory_info().rss / (1024 * 1024)


def run_benchmark(name, num_records, data_list, parse_fn):
    """Run a parsing benchmark and return metrics."""
    # Force GC and measure baseline memory
    gc.collect()
    start_mem = get_memory_usage()

    start_time = time.perf_counter()

    # Run the parsing
    tapes = []
    for data in data_list:
        tape = parse_fn(data)
        # We need to keep some reference to prevent immediate drop if we want to measure concurrent use
        # but for churn, we usually drop them quickly.
        # However, to measure "peak" or "delta", let's simulate a small window of liveness
        tapes.append(tape)
        if len(tapes) > 100:
            tapes.pop(0)  # Keep a sliding window of 100 tapes

    end_time = time.perf_counter()
    end_mem = get_memory_usage()

    duration = end_time - start_time
    throughput = num_records / duration
    mem_delta = end_mem - start_mem

    print(f"[{name}]")
    print(f"  Duration:   {duration:.4f}s")
    print(f"  Throughput: {throughput:.2f} ops/s")
    print(f"  Mem Delta:  {mem_delta:.4f} MB")
    print()

    return {"name": name, "duration": duration, "throughput": throughput, "mem_delta": mem_delta}


def main():
    num_records = 500_000
    print(f"Generating {num_records} JSON records...")

    # Create larger JSON objects
    data_list = [
        f'{{"id": {i}, "data": "{"X" * 1024}", "val": {i * 0.5}}}'.encode()
        for i in range(num_records)
    ]

    print("Starting benchmarks...\n")

    # 1. Baseline: Standard parse_tape (allocates every time)
    results_baseline = run_benchmark(
        "Standard (No Pooling)", num_records, data_list, lambda d: fx.Tape.parse(d)
    )

    # 2. Pooled: TapePool.parse (reuses buffers)
    pool = fx.TapePool(strategy="lru", max_tapes=128)
    results_pooled = run_benchmark(
        "Native Pooling", num_records, data_list, lambda d: pool.parse(d)
    )

    print(f"Final Pool Stats: {pool!r}")

    speedup = results_pooled["throughput"] / results_baseline["throughput"]
    print(f"Pooling Speedup: {speedup:.2f}x")


if __name__ == "__main__":
    main()
