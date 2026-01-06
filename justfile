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
# Fuzz Testing
# =============================================================================
# Two fuzzing backends supported:
#   - AFL (cargo-afl): Local fuzzing with AFL instrumentation
#   - libFuzzer (cargo-fuzz): OSS-Fuzz compatible, requires nightly
#
# AFL targets:     fuzz/targets/fuzz_*.rs      (6 targets)
# libFuzzer targets: fuzz/fuzz_targets/*.rs    (OSS-Fuzz compatible)
# =============================================================================

# -----------------------------------------------------------------------------
# AFL Fuzzing (requires: cargo install cargo-afl)
# -----------------------------------------------------------------------------

# Build all AFL fuzz targets
afl-build:
    cargo afl build --release --features afl-fuzz --bin fuzz_tape
    cargo afl build --release --features afl-fuzz --bin fuzz_path
    cargo afl build --release --features afl-fuzz --bin fuzz_jsonl
    cargo afl build --release --features afl-fuzz --bin fuzz_gron
    cargo afl build --release --features afl-fuzz --bin fuzz_diff
    cargo afl build --release --features afl-fuzz --bin fuzz_classify

# Run AFL fuzz target (usage: just afl-fuzz tape)
afl-fuzz target:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    mkdir -p fuzz/output/{{target}}
    cargo afl build --release --features afl-fuzz --bin fuzz_{{target}}
    cargo afl fuzz -i fuzz/corpus/{{target}} -o fuzz/output/{{target}} target/release/fuzz_{{target}}

# Run AFL fuzz target for specified duration (usage: just afl-timed tape 5m)
afl-timed target duration:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    mkdir -p fuzz/output/{{target}}
    cargo afl build --release --features afl-fuzz --bin fuzz_{{target}}
    cargo afl fuzz -i fuzz/corpus/{{target}} -o fuzz/output/{{target}} -V {{duration}} target/release/fuzz_{{target}}

# Quick AFL fuzz all targets (1 minute each)
afl-quick:
    #!/usr/bin/env bash
    set -e
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
    for target in tape path jsonl gron diff classify; do
        echo "=== AFL fuzzing $target for 1 minute ==="
        mkdir -p fuzz/output/$target
        cargo afl build --release --features afl-fuzz --bin fuzz_$target
        timeout 60 cargo afl fuzz -i fuzz/corpus/$target -o fuzz/output/$target target/release/fuzz_$target || true
    done
    echo "=== AFL fuzz quick complete ==="

# -----------------------------------------------------------------------------
# libFuzzer / OSS-Fuzz (requires: rustup install nightly && cargo install cargo-fuzz)
# -----------------------------------------------------------------------------

# List available libFuzzer targets
libfuzzer-list:
    cargo +nightly fuzz list

# Build all libFuzzer targets
libfuzzer-build:
    cargo +nightly fuzz build

# Run libFuzzer target (usage: just libfuzzer-run fuzz_tape_libfuzzer)
libfuzzer-run target:
    cargo +nightly fuzz run {{target}}

# Run libFuzzer target for duration (usage: just libfuzzer-timed fuzz_tape_libfuzzer 60)
libfuzzer-timed target seconds:
    cargo +nightly fuzz run {{target}} -- -max_total_time={{seconds}}

# Quick libFuzzer run (30 seconds)
libfuzzer-quick target:
    cargo +nightly fuzz run {{target}} -- -max_total_time=30

# Minimize a crash artifact (usage: just libfuzzer-tmin fuzz_tape_libfuzzer artifacts/...)
libfuzzer-tmin target artifact:
    cargo +nightly fuzz tmin {{target}} {{artifact}}

# Show libFuzzer coverage
libfuzzer-coverage target:
    cargo +nightly fuzz coverage {{target}}

# -----------------------------------------------------------------------------
# Unified Fuzz Commands
# -----------------------------------------------------------------------------

# Check for crashes in all fuzz output directories
fuzz-crashes:
    #!/usr/bin/env bash
    echo "=== Checking for AFL crashes ==="
    find fuzz/output -name "crashes" -type d -exec sh -c 'echo "--- {} ---"; ls -la "{}" 2>/dev/null | grep -v "^total\|^d\|README" || echo "(empty)"' \;
    echo ""
    echo "=== Checking for libFuzzer crashes ==="
    if [ -d "fuzz/artifacts" ]; then
        find fuzz/artifacts -name "crash-*" -o -name "oom-*" -o -name "timeout-*" 2>/dev/null | head -20 || echo "(none)"
    else
        echo "(no artifacts directory)"
    fi

# Clean all fuzz output
fuzz-clean:
    rm -rf fuzz/output/*
    rm -rf fuzz/artifacts/*
    rm -rf fuzz/corpus/fuzz_*_libfuzzer

# Clean only libFuzzer build artifacts (reclaim disk space)
fuzz-clean-build:
    rm -rf fuzz/target

# Aliases for backwards compatibility
fuzz-build: afl-build
fuzz target: (afl-fuzz target)
fuzz-timed target duration: (afl-timed target duration)
fuzz-quick: afl-quick
