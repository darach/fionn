# SPDX-License-Identifier: MIT OR Apache-2.0
"""
Multi-threaded benchmark for measuring pooling benefits under concurrent load.
"""

import gc
import os
import threading
import time

import fionn.ext as fx
import psutil


def get_memory_usage():
    """Get current RSS memory usage in MB."""
    process = psutil.Process(os.getpid())
    return process.memory_info().rss / (1024 * 1024)


def worker(data_list, parse_fn, results, index):
    start_time = time.perf_counter()
    count = 0
    tapes = []
    for data in data_list:
        tape = parse_fn(data)
        tapes.append(tape)
        if len(tapes) > 10:
            tapes.pop(0)
        count += 1
    duration = time.perf_counter() - start_time
    results[index] = count / duration


def run_mt_benchmark(name, num_threads, records_per_thread, data_list, parse_fn):
    print(f"Running MT benchmark: {name} ({num_threads} threads)...")
    gc.collect()
    start_mem = get_memory_usage()

    threads = []
    results = [0.0] * num_threads

    start_time = time.perf_counter()
    for i in range(num_threads):
        t = threading.Thread(target=worker, args=(data_list, parse_fn, results, i))
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    duration = time.perf_counter() - start_time
    end_mem = get_memory_usage()

    total_throughput = sum(results)

    print(f"[{name}]")
    print(f"  Threads:    {num_threads}")
    print(f"  Duration:   {duration:.4f}s")
    print(f"  Throughput: {total_throughput:.2f} total ops/s")
    print(f"  Mem Delta:  {end_mem - start_mem:.4f} MB")
    print()

    return total_throughput


def main():
    num_threads = 4
    records_per_thread = 50_000

    print(f"Generating data for {num_threads} threads, {records_per_thread} records each...")
    data_list = [
        f'{{"id": {i}, "data": "{"X" * 1024}"}}'.encode() for i in range(records_per_thread)
    ]

    # 1. Baseline: No Pooling
    # We use a fresh parse function that allocates every time
    tp_baseline = run_mt_benchmark(
        "Standard (No Pooling)",
        num_threads,
        records_per_thread,
        data_list,
        lambda d: fx.Tape.parse(d),
    )

    # 2. Pooled: Shared TapePool
    pool = fx.TapePool(strategy="lru", max_tapes=512)
    tp_pooled = run_mt_benchmark(
        "Native Pooling (Shared)",
        num_threads,
        records_per_thread,
        data_list,
        lambda d: pool.parse(d),
    )

    print(f"Multi-threaded Speedup: {tp_pooled / tp_baseline:.2f}x")


if __name__ == "__main__":
    main()
