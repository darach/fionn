import json
import random
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

try:
    import fionn
    import fionn.ext as fx

    HAS_FIONN = True
except ImportError:
    HAS_FIONN = False


def generate_record(i, wide=False):
    record = {
        "event_id": str(uuid.uuid4()),
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "event_type": random.choice(["click", "view", "purchase", "login", "logout"]),
        "user_id": random.randint(1000, 9999),
        "score": random.uniform(0, 100),
        "is_active": random.choice([True, False]),
    }

    if wide:
        # Add 20 extra fields to simulate 'Wide' data
        for j in range(20):
            record[f"field_{j}"] = f"value_{random.randint(0, 1000)}"

    # Add nested data
    record["payload"] = {
        "browser": random.choice(["chrome", "firefox", "safari"]),
        "os": random.choice(["linux", "macos", "windows"]),
        "version": f"{random.randint(1, 120)}.0.0",
    }
    record["metadata"] = [random.randint(0, 100) for _ in range(3)]

    return record


def main(num_records=100000, output_dir="data", wide=True):
    out_path = Path(output_dir)
    out_path.mkdir(exist_ok=True)

    jsonl_file = out_path / "events_wide.jsonl"
    isonl_file = out_path / "events_wide.isonl"
    csv_file = out_path / "events.csv"

    print(f"Generating {num_records} wide records to {jsonl_file}...")
    start_time = time.time()

    with open(jsonl_file, "w") as f:
        for i in range(num_records):
            record = generate_record(i, wide=wide)
            if HAS_FIONN:
                f.write(fionn.dumps(record).decode() + "\n")
            else:
                f.write(json.dumps(record) + "\n")

    gen_time = time.time() - start_time
    print(f"Generated JSONL in {gen_time:.2f}s")

    if HAS_FIONN:
        print(f"Converting JSONL to ISONL ({isonl_file})...")
        start_time = time.time()
        count = fx.jsonl_to_isonl(
            str(jsonl_file), str(isonl_file), table="events", infer_schema=True
        )
        conv_time = time.time() - start_time
        print(f"Converted {count} records to ISONL in {conv_time:.2f}s")

        # Generate CSV using fionn's string to_csv or manual
        print(f"Generating CSV ({csv_file})...")
        with open(csv_file, "w") as f:
            f.write("event_id,timestamp,event_type,user_id,score\n")
            # Just generate a few lines manually
            for _ in range(1000):
                r = generate_record(0)
                f.write(
                    f"{r['event_id']},{r['timestamp']},{r['event_type']},{r['user_id']},{r['score']}\n"
                )

        # Generate GRON/CRDT specialized data
        generate_specialized_data(out_path, num_records=num_records // 10)
    else:
        print("fionn not found, skipping ISONL/CSV generation")


def generate_specialized_data(out_path, num_records=1000):
    print(f"Generating Specialized Data ({num_records} records for GRON & CRDT)...")

    # 1. Bulk Deeply Nested for GRON
    gron_jsonl = out_path / "events_nested.jsonl"
    with open(gron_jsonl, "w") as f:
        for i in range(num_records):
            # 15 levels of nesting to stress test Python recursion vs Rust Tape
            nested_data = {"id": f"event_{i}"}
            curr = nested_data
            for _j in range(15):
                curr["node"] = {"val": random.random()}
                curr = curr["node"]
            f.write(json.dumps(nested_data) + "\n")

    # 2. Bulk CRDT Conflicts Scenario
    profile_a_jsonl = out_path / "profiles_a.jsonl"
    profile_b_jsonl = out_path / "profiles_b.jsonl"

    with open(profile_a_jsonl, "w") as fa, open(profile_b_jsonl, "w") as fb:
        for i in range(num_records):
            user_id = 1000 + i
            # Source A: Lower points, some fields
            p_a = {
                "user_id": user_id,
                "name": f"User_{user_id}_A",
                "points": random.randint(0, 1000),
                "ts": int(time.time() * 1000) - 1000,  # Slightly older
            }
            # Source B: Higher points, other fields
            p_b = {
                "user_id": user_id,
                "name": f"User_{user_id}_B",
                "points": random.randint(1001, 2000),
                "location": "Global",
                "ts": int(time.time() * 1000),
            }
            fa.write(json.dumps(p_a) + "\n")
            fb.write(json.dumps(p_b) + "\n")

    # 3. Conflict Storm Bulk JSONL
    storm_file = out_path / "storm_updates.jsonl"
    print(f"Generating Conflict Storm data ({num_records * 10} updates)...")
    with open(storm_file, "w") as f:
        for i in range(num_records * 10):
            update = {f"field_{random.randint(0, 19)}": random.randint(0, 1000000), "ts": i}
            f.write(json.dumps(update) + "\n")


if __name__ == "__main__":
    import sys

    count = 100000
    if len(sys.argv) > 1:
        count = int(sys.argv[1])
    main(count)
