# Default recipe
default:
    @just --list

# Build
build:
    cargo build

build-release:
    cargo build --release

build-all-features:
    cargo build --all-features

# Test
test:
    cargo test

test-release:
    cargo test --release

test-verbose:
    cargo test -- --nocapture

test-all-features:
    cargo test --all-features

# Lint and format
fmt:
    cargo fmt

fmt-check:
    cargo fmt -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

# Note: afl-fuzz feature excluded; requires AFL toolchain
clippy-all:
    cargo clippy --all-targets --features fionn-cli/gpu -- -D warnings

lint: fmt-check clippy

# Documentation
doc:
    cargo doc

doc-open:
    cargo doc --open

# Note: afl-fuzz feature excluded; requires AFL toolchain
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --features fionn-cli/gpu

# Clean
clean:
    cargo clean

# Benchmarks
bench:
    cargo build --benches

# Feature-specific builds
# Note: Features are not currently defined in the workspace

# Coverage with cargo-llvm-cov (requires cargo-llvm-cov to be installed)
coverage:
    cargo llvm-cov

coverage-html:
    cargo llvm-cov --html

coverage-lcov:
    cargo llvm-cov --lcov --output-path lcov.info

coverage-open:
    cargo llvm-cov --html --open

coverage-all-features:
    cargo llvm-cov --all-features

coverage-clean:
    cargo llvm-cov clean

# Security audit (requires cargo-audit and cargo-deny)
audit:
    cargo audit
    cargo deny check

# CI-style check (runs fmt, clippy, doc, tests, security audit)
ci: fmt-check clippy doc-check test audit

# Full check with all features
ci-full: fmt-check clippy-all doc-check test-all-features audit

# Run GPU binary
run-gpu:
    cargo run --bin gpu-jsonl --release

# =============================================================================
# Fuzz Testing (requires cargo-afl: cargo install cargo-afl)
# =============================================================================

# Build all fuzz targets
fuzz-build:
    cargo afl build --release --features afl-fuzz --bin fuzz_tape
    cargo afl build --release --features afl-fuzz --bin fuzz_path
    cargo afl build --release --features afl-fuzz --bin fuzz_jsonl
    cargo afl build --release --features afl-fuzz --bin fuzz_gron
    cargo afl build --release --features afl-fuzz --bin fuzz_diff
    cargo afl build --release --features afl-fuzz --bin fuzz_classify

# Run fuzz target (usage: just fuzz tape)
fuzz target:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    mkdir -p fuzz/output/{{target}}
    cargo afl build --release --features afl-fuzz --bin fuzz_{{target}}
    cargo afl fuzz -i fuzz/corpus/{{target}} -o fuzz/output/{{target}} target/release/fuzz_{{target}}

# Run fuzz target for specified duration (usage: just fuzz-timed tape 5m)
fuzz-timed target duration:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    mkdir -p fuzz/output/{{target}}
    cargo afl build --release --features afl-fuzz --bin fuzz_{{target}}
    cargo afl fuzz -i fuzz/corpus/{{target}} -o fuzz/output/{{target}} -V {{duration}} target/release/fuzz_{{target}}

# Quick fuzz all targets (1 minute each)
fuzz-quick:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    for target in tape path jsonl gron diff classify; do
        echo "=== Fuzzing $target for 1 minute ==="
        mkdir -p fuzz/output/$target
        cargo afl build --release --features afl-fuzz --bin fuzz_$target
        timeout 60 cargo afl fuzz -i fuzz/corpus/$target -o fuzz/output/$target target/release/fuzz_$target || true
    done
    echo "=== Fuzz quick complete ==="

# Check for crashes in fuzz output
fuzz-crashes:
    #!/usr/bin/env bash
    echo "=== Checking for fuzz crashes ==="
    find fuzz/output -name "crashes" -type d -exec sh -c 'echo "--- {} ---"; ls -la "{}" 2>/dev/null | grep -v "^total\|^d\|README" || echo "(empty)"' \;

# Clean fuzz output
fuzz-clean:
    rm -rf fuzz/output/*
