import json
import subprocess
import sys
import time
from pathlib import Path

import dlt

# Path to pure Rust fionn CLI
FIONN_CLI = Path(__file__).parent.parent.parent.parent.parent / "target" / "release" / "fionn"

# Try to import fionn
try:
    import fionn
    import fionn.ext as fx

    HAS_FIONN = True
except ImportError:
    HAS_FIONN = False

# Try to import orjson
try:
    import orjson

    HAS_ORJSON = True
except ImportError:
    HAS_ORJSON = False

# Try to import ujson
try:
    import ujson

    HAS_UJSON = True
except ImportError:
    HAS_UJSON = False

# Try to import simdjson
try:
    import simdjson

    HAS_SIMDJSON = True
except ImportError:
    HAS_SIMDJSON = False


def get_memory_usage():
    """Get current RSS in MB."""
    try:
        with open("/proc/self/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1]) / 1024.0
    except:
        return 0.0
    return 0.0


def measure_memory_delta(func, *args, **kwargs):
    """Measure memory delta during function execution."""
    import gc

    gc.collect()
    mem_before = get_memory_usage()
    result = func(*args, **kwargs)
    gc.collect()
    mem_after = get_memory_usage()
    return result, max(0, mem_after - mem_before)


def run_baseline(jsonl_path):
    """Run DLT pipeline using standard JSONL reading."""
    print(f"Running Baseline DLT with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_baseline", destination="duckdb", dataset_name="events_data"
    )

    # Standard way: DLT will read the file
    # By default, dlt.resource can take a file path
    @dlt.resource(name="events")
    def events_resource():
        import json

        with open(jsonl_path) as f:
            for line in f:
                yield json.loads(line)

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_baseline_selective(jsonl_path, fields):
    """Run DLT pipeline using standard JSONL with manual field extraction."""
    print(f"Running Baseline Selective DLT with {len(fields)} fields...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_baseline_selective",
        destination="duckdb",
        dataset_name="events_data_selective",
    )

    @dlt.resource(name="events_selective")
    def events_resource():
        with open(jsonl_path) as f:
            for line in f:
                record = json.loads(line)
                # Extract only specified fields
                yield {k: record.get(k) for k in fields}

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_baseline_csv(csv_path):
    """Run DLT pipeline reading CSV with stdlib csv module."""
    import csv

    print(f"Running Baseline CSV DLT with {csv_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_baseline_csv",
        destination="duckdb",
        dataset_name="events_data_csv",
    )

    @dlt.resource(name="events_csv")
    def events_resource():
        with open(csv_path) as f:
            reader = csv.DictReader(f)
            yield from reader

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_orjson(jsonl_path):
    """Run DLT pipeline using orjson."""
    if not HAS_ORJSON:
        return None, "orjson not installed"

    print(f"Running orjson DLT with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_orjson", destination="duckdb", dataset_name="events_data"
    )

    @dlt.resource(name="events")
    def events_resource():
        with open(jsonl_path, "rb") as f:
            for line in f:
                yield orjson.loads(line)

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_ujson(jsonl_path):
    """Run DLT pipeline using ujson."""
    if not HAS_UJSON:
        return None, "ujson not installed"

    print(f"Running ujson DLT with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_ujson", destination="duckdb", dataset_name="events_data"
    )

    @dlt.resource(name="events")
    def events_resource():
        with open(jsonl_path) as f:
            for line in f:
                yield ujson.loads(line)

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_simdjson(jsonl_path):
    """Run DLT pipeline using pysimdjson."""
    if not HAS_SIMDJSON:
        return None, "simdjson not installed"

    print(f"Running simdjson DLT with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_simdjson", destination="duckdb", dataset_name="events_data"
    )

    parser = simdjson.Parser()

    @dlt.resource(name="events")
    def events_resource():
        with open(jsonl_path, "rb") as f:
            for line in f:
                yield parser.parse(line).as_dict()

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_accelerated(jsonl_path):
    """Run DLT pipeline using fionn as a drop-in loads replacement."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running Accelerated DLT (fionn.loads) with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_accelerated", destination="duckdb", dataset_name="events_data"
    )

    @dlt.resource(name="events")
    def events_resource():
        # Using fionn.loads which is SIMD accelerated
        with open(jsonl_path, "rb") as f:
            for line in f:
                yield fionn.loads(line)

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_native_jsonl(jsonl_path):
    """Run DLT pipeline using fionn.ext.JsonlReader (Native Mode)."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running Native JSONL DLT (fionn.ext.JsonlReader) with {jsonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_native_jsonl", destination="duckdb", dataset_name="events_data"
    )

    @dlt.resource(name="events")
    def events_resource():
        # Using native JsonlReader which handles looping and parsing in Rust
        reader = fx.JsonlReader(str(jsonl_path), batch_size=5000)
        yield from reader

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()
    return end - start, load_info


def run_selective_jsonl(jsonl_path, fields):
    """Run DLT pipeline using fionn.ext.JsonlReader with selective schema."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running Selective JSONL DLT with {len(fields)} fields...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_selective_jsonl",
        destination="duckdb",
        dataset_name="events_data_selective",
    )

    @dlt.resource(name="events_selective")
    def events_resource():
        # Native schema filtering at parse time
        reader = fx.JsonlReader(str(jsonl_path), schema=fields, batch_size=5000)
        yield from reader

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_selective_isonl(isonl_path, fields):
    """Run DLT pipeline using fionn.ext.IsonlReader with selective fields."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running Selective ISONL DLT with {len(fields)} fields...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_selective_isonl",
        destination="duckdb",
        dataset_name="events_data_selective",
    )

    @dlt.resource(name="events_selective")
    def events_resource():
        # SIMD-accelerated field extraction
        reader = fx.IsonlReader(str(isonl_path), fields=fields, batch_size=5000)
        yield from reader

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_csv_to_isonl_ingest(csv_path):
    """Demonstrate fionn Agile Format Agility: CSV -> ISONL -> DLT."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running CSV -> ISONL Agility Demo with {csv_path}...")

    # Step 1: Agility Translation (Agile Data Engineering)
    start_trans = time.time()
    with open(csv_path) as f:
        csv_content = f.read()

    # Convert CSV string to Python, then to ISONL
    py_data = fx.parse_csv(csv_content)
    isonl_content = fx.to_isonl(
        py_data, table="events", schema=["event_id:string", "user_id:int", "score:float"]
    )

    temp_isonl = Path("temp_agility.isonl")
    with open(temp_isonl, "w") as f:
        f.write(isonl_content)
    trans_time = time.time() - start_trans
    print(f"Translated CSV to ISONL in {trans_time:.4f}s")

    # Step 2: High Performance Ingest
    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_agility", destination="duckdb", dataset_name="events_data_agility"
    )

    @dlt.resource(name="agility_events")
    def agility_resource():
        yield from fx.IsonlReader(str(temp_isonl))

    start_ingest = time.time()
    load_info = pipeline.run(agility_resource())
    end_ingest = time.time()

    return (end_ingest - start_ingest) + trans_time, load_info


def run_python_flattening(jsonl_path):
    """Baseline: Recursive Python flattening of nested JSONL."""
    print(f"Running Python Flattening Baseline with {jsonl_path}...")

    def flatten_dict(d, parent_key="", sep="."):
        items = []
        for k, v in d.items():
            new_key = f"{parent_key}{sep}{k}" if parent_key else k
            if isinstance(v, dict):
                items.extend(flatten_dict(v, new_key, sep=sep).items())
            else:
                items.append((new_key, v))
        return dict(items)

    start = time.time()
    processed = []
    with open(jsonl_path) as f:
        for line in f:
            data = json.loads(line)
            processed.append(flatten_dict(data))

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_python_gron", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(processed, name="python_flat"))
    end = time.time()
    return end - start, load_info


def run_gron_flattening(jsonl_path):
    """Fionn: Native GRON flattening of nested JSONL."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    print(f"Running Fionn GRON Flattening with {jsonl_path}...")

    start = time.time()
    processed = []
    with open(jsonl_path) as f:
        for line in f:
            # Native GRON -> Ungron cycle actually performs high-speed flattening
            gron_str = fx.gron(line)
            processed.append(fx.ungron(gron_str))

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_fionn_gron", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(processed, name="fionn_flat"))
    end = time.time()
    return end - start, load_info


def run_python_merge(path_a, path_b):
    """Baseline: Simple Python dict merge for conflicts."""
    print(f"Running Python Merge Baseline with {path_a} and {path_b}...")

    start = time.time()
    merged_list = []
    with open(path_a) as fa, open(path_b) as fb:
        for la, lb in zip(fa, fb):
            da = json.loads(la)
            db = json.loads(lb)
            # Manual 'LWW' merge logic in Python
            entry = da.copy()
            for k, v in db.items():
                if k not in entry or db.get("ts", 0) > entry.get("ts", 0):
                    entry[k] = v
            merged_list.append(entry)

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_python_crdt", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(merged_list, name="python_crdt"))
    end = time.time()
    return end - start, load_info


def run_crdt_merge(path_a, path_b):
    """Fionn: Native CRDT merging."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    print(f"Running Fionn CRDT Merge with {path_a} and {path_b}...")

    start = time.time()
    merged_list = []
    with open(path_a) as fa, open(path_b) as fb:
        for la, lb in zip(fa, fb):
            # Using CRDT document for conflict resolution
            doc = fx.CrdtDocument(json.loads(la), replica_id="A")
            remote = fx.CrdtDocument(json.loads(lb), replica_id="B")
            doc.set_strategy("points", fx.MergeStrategy.Max)
            doc.merge(remote)
            merged_list.append(doc.value)

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_fionn_crdt", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(merged_list, name="fionn_crdt"))
    end = time.time()
    return end - start, load_info


def run_parallel_crdt_merge(path_a, path_b, workers=4):
    """Fionn: Parallel Native CRDT merging (GIL-free)."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    from concurrent.futures import ThreadPoolExecutor

    print(f"Running Parallel Fionn CRDT Merge ({workers} workers) with {path_a} and {path_b}...")

    def merge_pair(la, lb):
        doc = fx.CrdtDocument(json.loads(la), replica_id="A")
        remote = fx.CrdtDocument(json.loads(lb), replica_id="B")
        doc.set_strategy("points", fx.MergeStrategy.Max)
        doc.merge(remote)
        return doc.value

    start = time.time()
    with open(path_a) as fa, open(path_b) as fb:
        lines_a = fa.readlines()
        lines_b = fb.readlines()

    with ThreadPoolExecutor(max_workers=workers) as executor:
        merged_list = list(executor.map(merge_pair, lines_a, lines_b))

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_parallel_crdt", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(merged_list, name="parallel_crdt"))
    end = time.time()
    return end - start, load_info


def run_conflict_storm(num_updates=20000, workers=4):
    """Conflict Storm: Multiple threads hammering a Wide CRDT document (GIL-free)."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    import random
    from concurrent.futures import ThreadPoolExecutor

    print(f"Running Fionn Conflict Storm (CrdtDocument wrapper, {workers} workers)...")

    # Base state with 20 fields
    initial = {f"field_{i}": 0 for i in range(20)}
    doc = fx.CrdtDocument(initial, replica_id="leader")
    for i in range(20):
        doc.set_strategy(f"field_{i}", fx.MergeStrategy.Max)

    def worker_task(worker_id):
        updates = []
        for i in range(num_updates // workers):
            update = {f"field_{j}": random.randint(0, 1000000) for j in range(20)}
            updates.append(fx.CrdtDocument(update, replica_id=f"worker-{worker_id}-{i}"))

        for remote in updates:
            doc.merge(remote)

    start = time.time()
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = [executor.submit(worker_task, i) for i in range(workers)]
        for f in futures:
            f.result()
    end = time.time()

    return end - start, doc.value


def run_conflict_storm_native(jsonl_path):
    """Conflict Storm: Native Rust JSONL stream merge (Ultra Juice)."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    print("Running Native Fionn Conflict Storm (JSONL Stream)...")

    initial = {f"field_{i}": 0 for i in range(20)}
    doc = fx.CrdtDocument(initial, replica_id="leader")
    for i in range(20):
        doc.set_strategy(f"field_{i}", fx.MergeStrategy.Max)

    start = time.time()
    doc.merge_jsonl(str(jsonl_path))
    end = time.time()

    # Materialize to DLT for verification
    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_native_crdt", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource([doc.value], name="native_crdt"))

    return end - start, load_info


def run_python_conflict_storm(num_updates=20000, workers=4):
    """Conflict Storm: Python baseline (GIL-bound) with Wide Data."""
    import random
    import threading
    from concurrent.futures import ThreadPoolExecutor

    print("Running Python Conflict Storm Baseline (Wide Data)...")

    state = {f"field_{i}": 0 for i in range(20)}
    lock = threading.Lock()

    def worker_task():
        for _i in range(num_updates // workers):
            # Dense updates
            update = {f"field_{j}": random.randint(0, 1000000) for j in range(20)}
            with lock:
                # Manual Max merge for each field
                for k, v in update.items():
                    state[k] = max(state[k], v)

    start = time.time()
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = [executor.submit(worker_task) for i in range(workers)]
        for f in futures:
            f.result()
    end = time.time()

    return end - start, state


def run_filtering_baseline(jsonl_path, query_key, query_val):
    """Filter rows in Python baseline."""
    print(f"Running Filtering Baseline (Python) on {jsonl_path} for {query_key}=={query_val}...")

    start = time.time()
    matches = []
    with open(jsonl_path) as f:
        for line in f:
            data = json.loads(line)
            if str(data.get(query_key)) == str(query_val):
                matches.append(data)

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_filter_baseline", destination="duckdb", dataset_name="advanced"
    )
    load_info = pipeline.run(dlt.resource(matches, name="python_filtered"))
    end = time.time()
    return end - start, load_info


def run_filtering_fionn(jsonl_path, query_key, query_val):
    """Filter rows natively in Fionn JsonlReader."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    print(f"Running Filtering Fionn (Native) on {jsonl_path} for {query_key}=={query_val}...")

    start = time.time()
    # Using the new native filter parameter
    reader = fx.JsonlReader(str(jsonl_path), filter=(query_key, str(query_val)), batch_size=5000)

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_filter_fionn", destination="duckdb", dataset_name="advanced"
    )

    def filtered_gen():
        yield from reader

    load_info = pipeline.run(dlt.resource(filtered_gen, name="fionn_filtered"))
    end = time.time()
    return end - start, load_info


def run_memory_pressure_baseline(jsonl_path):
    """DOM-based: Load everything into memory as dicts."""
    print(f"Running Memory Pressure (DOM/Dict) on {jsonl_path}...")
    start = time.time()
    all_records = []
    with open(jsonl_path) as f:
        for line in f:
            all_records.append(json.loads(line))

    # Simulate some processing to hold memory
    total_fields = sum(len(r) for r in all_records)
    print(f"Loaded {len(all_records)} records with {total_fields} total field entries.")

    end = time.time()
    return end - start, f"Loaded {len(all_records)} into memory"


def run_memory_pressure_fionn(jsonl_path):
    """Streaming: Scan everything with minimal materialization."""
    print(f"Running Memory Pressure (Streaming) on {jsonl_path}...")
    start = time.time()
    if not HAS_FIONN:
        return None, "fionn not installed"

    reader = fx.JsonlReader(str(jsonl_path), batch_size=1000)
    total_count = 0
    total_fields = 0
    for batch in reader:
        total_count += len(batch)
        for r in batch:
            total_fields += len(r)

    end = time.time()
    return end - start, f"Streamed {total_count} records"


def run_ultra(isonl_path):
    """Run DLT pipeline using fionn.ext.IsonlReader (Ultra Mode)."""
    if not HAS_FIONN:
        return None, "fionn not installed"

    print(f"Running Ultra DLT (fionn ISONL) with {isonl_path}...")

    pipeline = dlt.pipeline(
        pipeline_name="fionn_demo_ultra", destination="duckdb", dataset_name="events_data"
    )

    @dlt.resource(name="events")
    def events_resource():
        # IsonlReader returns batches, we flat-map them or yield them directly
        # (dlt handles lists of dicts efficiently)
        reader = fx.IsonlReader(str(isonl_path), batch_size=5000)
        yield from reader

    start = time.time()
    load_info = pipeline.run(events_resource())
    end = time.time()

    return end - start, load_info


def run_pooling_baseline(num_records=100000):
    """Baseline: Parse many small JSON strings without pooling."""
    import json

    print(f"Running Pooling Baseline (No Pool, {num_records} iterations)...")

    # Create some dummy data
    data_str = json.dumps({"id": 1, "name": "fionn", "tags": ["rust", "python", "simd"]})

    start = time.time()
    for _ in range(num_records):
        _ = json.loads(data_str)
    end = time.time()

    return end - start, f"Parsed {num_records} objects"


def run_pooling_fionn(num_records=100000):
    """Fionn: Parse many small JSON strings using TapePool (Native Pooling)."""
    if not HAS_FIONN:
        return None, "fionn not installed"
    print(f"Running Pooling Fionn (TapePool, {num_records} iterations)...")

    # Create some dummy data
    data_str = json.dumps({"id": 1, "name": "fionn", "tags": ["rust", "python", "simd"]})
    data_bytes = data_str.encode("utf-8")

    pool = fx.TapePool(max_tapes=100)  # Small pool for high churn

    start = time.time()
    for _ in range(num_records):
        _ = pool.parse(data_bytes)
    end = time.time()

    return end - start, f"Parsed {num_records} objects with pooling"


# =============================================================================
# PURE RUST (fionn CLI) BENCHMARKS - No Python binding overhead
# =============================================================================


def run_rust_cli_stream(jsonl_path):
    """Pure Rust: fionn CLI stream command (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI stream on {jsonl_path}...")

    start = time.time()
    result = subprocess.run(
        [str(FIONN_CLI), "stream", "-q", "-o", "/dev/null", jsonl_path], capture_output=True
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust stream"


def run_rust_cli_stream_selective(jsonl_path, fields):
    """Pure Rust: fionn CLI stream with field selection (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    fields_str = ",".join(fields)
    print(f"Running Pure Rust fionn CLI stream with fields={fields_str}...")

    start = time.time()
    result = subprocess.run(
        [str(FIONN_CLI), "stream", "-q", "-F", fields_str, "-o", "/dev/null", jsonl_path],
        capture_output=True,
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, f"Pure Rust selective ({fields_str})"


def run_rust_cli_gron(jsonl_path):
    """Pure Rust: fionn CLI stream to gron format (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI stream->gron on {jsonl_path}...")

    start = time.time()
    # Use stream command with gron output format for JSONL input
    result = subprocess.run(
        [str(FIONN_CLI), "stream", "-t", "gron", "-o", "/dev/null", jsonl_path], capture_output=True
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust stream->gron"


def run_rust_cli_convert(csv_path):
    """Pure Rust: fionn CLI convert CSV to JSONL (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI convert CSV->JSONL on {csv_path}...")

    start = time.time()
    result = subprocess.run(
        [str(FIONN_CLI), "convert", "-q", "-f", "csv", "-t", "jsonl", "-o", "/dev/null", csv_path],
        capture_output=True,
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust CSV->JSONL"


def run_rust_cli_merge(file_a, file_b):
    """Pure Rust: fionn CLI stream two JSONL files (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI stream on {file_a} + {file_b}...")

    # Concatenate files and stream through fionn
    start = time.time()
    # Use shell pipeline: cat file_a file_b | fionn stream -o /dev/null
    result = subprocess.run(
        f"cat {file_a} {file_b} | {FIONN_CLI!s} stream -o /dev/null",
        shell=True,
        capture_output=True,
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust stream (2 files)"


def run_rust_cli_merge_stream(jsonl_path):
    """Pure Rust: fionn CLI stream processing (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI stream on {jsonl_path}...")

    # Pure stream processing - output to /dev/null
    start = time.time()
    result = subprocess.run(
        [str(FIONN_CLI), "stream", "-o", "/dev/null", jsonl_path], capture_output=True
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust stream"


def run_rust_cli_filter(jsonl_path, field, value):
    """Pure Rust: fionn CLI stream with filter (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI filter {field}=={value} on {jsonl_path}...")

    start = time.time()
    # Use stream with filter query
    result = subprocess.run(
        [
            str(FIONN_CLI),
            "stream",
            "--filter",
            f'.{field} == "{value}"',
            "-o",
            "/dev/null",
            jsonl_path,
        ],
        capture_output=True,
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, f"Pure Rust filter ({field}=={value})"


def run_rust_cli_memory_stream(jsonl_path):
    """Pure Rust: fionn CLI stream for memory comparison (no Python overhead)."""
    if not FIONN_CLI.exists():
        return None, "fionn CLI not built"

    print(f"Running Pure Rust fionn CLI stream (memory test) on {jsonl_path}...")

    start = time.time()
    # Pure streaming - never loads all records into memory
    result = subprocess.run(
        [str(FIONN_CLI), "stream", "-o", "/dev/null", jsonl_path], capture_output=True
    )
    end = time.time()

    if result.returncode != 0:
        return None, f"CLI failed: {result.stderr.decode()}"

    return end - start, "Pure Rust stream (memory)"


if __name__ == "__main__":
    data_dir = Path("data")
    jsonl = data_dir / "events.jsonl"
    isonl = data_dir / "events.isonl"

    if not jsonl.exists():
        print("Data files not found. Run data_gen.py first.")
        sys.exit(1)

    t1, info1 = run_baseline(str(jsonl))
    print(f"Baseline took: {t1:.2f}s")

    t2, info2 = run_accelerated(str(jsonl))
    print(f"Accelerated (fionn.loads) took: {t2:.2f}s")

    t4, info4 = run_native_jsonl(str(jsonl))
    print(f"Native JSONL (fionn.ext.JsonlReader) took: {t4:.2f}s")

    if isonl.exists():
        t3, info3 = run_ultra(str(isonl))
        print(f"Ultra (fionn ISONL) took: {t3:.2f}s")
        print(f"Speedup vs Baseline: {t1 / t3:.2f}x")
