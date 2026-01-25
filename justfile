# Default recipe
default:
    @just --list

# =============================================================================
# Build
# =============================================================================

# Quick check (faster than build)
check:
    cargo check --workspace

check-all-features:
    cargo check --workspace --all-features

build:
    cargo build

build-release:
    cargo build --release

# Build with debug symbols for profiling (uses release-debug profile)
build-profile:
    cargo build --profile release-debug

build-all-features:
    cargo build --all-features

# Build specific crate (usage: just build-crate fionn-core)
build-crate crate:
    cargo build -p {{crate}}

# =============================================================================
# Test
# =============================================================================

test:
    cargo test

test-release:
    cargo test --release

test-verbose:
    cargo test -- --nocapture

test-all-features:
    cargo test --all-features

# Test specific crate (usage: just test-crate fionn-core)
test-crate crate:
    cargo test -p {{crate}}

# MSRV check (Rust 1.89)
msrv:
    cargo +1.89 check --workspace

# =============================================================================
# Lint and format
# =============================================================================

fmt:
    cargo fmt

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

# Clippy with all format features (afl-fuzz excluded; requires AFL toolchain)
clippy-all:
    cargo clippy --all-targets --features fionn-cli/all-formats -- -D warnings

# Auto-fix clippy warnings
clippy-fix:
    cargo clippy --fix --allow-dirty --allow-staged

# Auto-fix compiler warnings
fix:
    cargo fix --allow-dirty --allow-staged

lint: fmt-check clippy

# =============================================================================
# Documentation
# =============================================================================

doc:
    cargo doc

doc-open:
    cargo doc --open

# Doc check with warnings as errors (afl-fuzz excluded; requires AFL toolchain)
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --features fionn-cli/all-formats

# =============================================================================
# Clean
# =============================================================================

clean:
    cargo clean

# =============================================================================
# Benchmarks
# =============================================================================

# Build benchmarks
bench-build:
    cargo build --benches --all-features

# Run all benchmarks
bench:
    cargo bench --all-features

# Run specific benchmark (usage: just bench-run tape_source_benchmark)
bench-run name:
    cargo bench --bench {{name}} --all-features

# Run benchmark and save baseline (usage: just bench-baseline main)
bench-baseline name:
    cargo bench --all-features -- --noplot --save-baseline {{name}}

# Compare benchmarks (usage: just bench-compare main pr)
bench-compare baseline current:
    critcmp {{baseline}} {{current}} --threshold 15

# Run key benchmarks for CI comparison
bench-ci:
    cargo bench --bench tape_source_benchmark --all-features -- --noplot
    cargo bench --bench format_benchmarks --all-features -- --noplot
    cargo bench --bench diff_patch_merge_crdt --all-features -- --noplot
    cargo bench --bench streaming_formats --all-features -- --noplot

# =============================================================================
# Coverage (requires cargo-llvm-cov)
# =============================================================================

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

# =============================================================================
# Security and Dependencies
# =============================================================================

# Security audit (requires cargo-audit and cargo-deny)
audit:
    cargo audit
    cargo deny check

# Dependency tree
tree:
    cargo tree

# Dependency tree for specific crate
tree-crate crate:
    cargo tree -p {{crate}}

# Check for outdated dependencies (shows what cargo update would change)
outdated:
    cargo update --dry-run

# Update dependencies
update:
    cargo update

# Update specific dependency
update-dep dep:
    cargo update -p {{dep}}

# =============================================================================
# CI Checks
# =============================================================================

# CI-style check (runs fmt, clippy, doc, tests, security audit)
ci: fmt-check clippy doc-check test audit

# Full check with all features
ci-full: fmt-check clippy-all doc-check test-all-features audit

# =============================================================================
# Python Bindings (requires maturin)
# =============================================================================

# Build Python wheel
py-build:
    cd crates/fionn-py && maturin build --release

# Build and install Python package in current venv
py-develop:
    cd crates/fionn-py && maturin develop

# Build Python wheel with all features
py-build-full:
    cd crates/fionn-py && maturin build --release --features full

# =============================================================================
# Publishing
# =============================================================================

# Check which crates need publishing based on changes since last release
publish-check:
    #!/usr/bin/env bash
    set -e

    # Get the last release tag
    LAST_TAG=$(git tag --list 'v*' --sort=-version:refname | head -1)

    if [ -z "$LAST_TAG" ]; then
        echo "No previous release tags found. All crates would be new releases."
        echo ""
        echo "Workspace crates:"
        cargo metadata --no-deps --format-version 1 | \
            jq -r '.packages[] | "  \(.name) v\(.version)"'
        exit 0
    fi

    echo "Last release: $LAST_TAG"
    echo "Comparing changes since $LAST_TAG..."
    echo ""

    # Get list of workspace crates and their paths
    CRATES=$(cargo metadata --no-deps --format-version 1 | \
        jq -r '.packages[] | "\(.name)|\(.manifest_path | split("/") | .[:-1] | join("/"))"')

    echo "Crates with changes since $LAST_TAG:"
    echo "=========================================="

    CHANGED=0
    while IFS='|' read -r name path; do
        # Get relative path from repo root
        rel_path=${path#$(pwd)/}

        # Check if there are changes in this crate's directory
        if git diff --quiet "$LAST_TAG"..HEAD -- "$rel_path" 2>/dev/null; then
            : # No changes
        else
            CHANGED=1
            # Get current version from Cargo.toml
            version=$(cargo metadata --no-deps --format-version 1 | \
                jq -r ".packages[] | select(.name == \"$name\") | .version")

            # Count commits affecting this crate
            commit_count=$(git log --oneline "$LAST_TAG"..HEAD -- "$rel_path" | wc -l)

            # Analyze change types for semver hint
            changes=$(git log --oneline "$LAST_TAG"..HEAD -- "$rel_path")

            semver_hint="patch"
            if echo "$changes" | grep -qiE "(breaking|remove|delete|rename.*api|major)"; then
                semver_hint="MAJOR"
            elif echo "$changes" | grep -qiE "(feat|feature|add|new|enhancement)"; then
                semver_hint="minor"
            fi

            echo ""
            echo "  $name (v$version)"
            echo "    Path: $rel_path"
            echo "    Commits: $commit_count"
            echo "    Suggested bump: $semver_hint"
        fi
    done <<< "$CRATES"

    if [ $CHANGED -eq 0 ]; then
        echo "  (none - no crates have changes)"
    fi

    echo ""
    echo "=========================================="
    echo ""
    echo "Dependency order for publishing:"
    echo "  1. fionn-simd"
    echo "  2. fionn-core"
    echo "  3. fionn-tape"
    echo "  4. fionn-diff"
    echo "  5. fionn-crdt"
    echo "  6. fionn-ops"
    echo "  7. fionn-gron"
    echo "  8. fionn-pool"
    echo "  9. fionn-stream"
    echo "  10. fionn-cli"
    echo "  11. fionn"

# Dry-run publish to check for issues
publish-dry-run crate:
    cargo publish -p {{crate}} --dry-run --allow-dirty

# Dry-run publish all crates in dependency order
publish-dry-run-all:
    #!/usr/bin/env bash
    set -e
    echo "Dry-run publishing all crates in dependency order..."
    for crate in fionn-simd fionn-core fionn-tape fionn-diff fionn-crdt fionn-ops fionn-gron fionn-pool fionn-stream fionn-cli fionn; do
        echo "=== Checking $crate ==="
        cargo publish -p $crate --dry-run --allow-dirty || echo "Warning: $crate dry-run failed"
    done

# Check semver compatibility (requires cargo-semver-checks)
semver-check:
    cargo semver-checks --workspace

# Check semver for specific crate
semver-check-crate crate:
    cargo semver-checks -p {{crate}}

# Generate semver report with recommended next versions
semver-report:
    #!/usr/bin/env bash
    set -e
    RED='\033[0;31m'; YELLOW='\033[0;33m'; GREEN='\033[0;32m'; DIM='\033[0;90m'; NC='\033[0m'
    CRATES="fionn-simd fionn-core fionn-tape fionn-diff fionn-crdt fionn-ops fionn-gron fionn-pool fionn-stream fionn"
    LAST_TAG=$(git tag --list 'v*' --sort=-version:refname | head -1)

    bump_version() {
        local ver=$1 level=$2
        IFS='.' read -r major minor patch <<< "$ver"
        case $level in
            major) echo "$((major+1)).0.0" ;;
            minor) echo "$major.$((minor+1)).0" ;;
            patch) echo "$major.$minor.$((patch+1))" ;;
        esac
    }

    for crate in $CRATES; do
        version=$(cargo metadata --no-deps --format-version 1 2>/dev/null | jq -r ".packages[] | select(.name == \"$crate\") | .version")
        crate_path=$(cargo metadata --no-deps --format-version 1 2>/dev/null | jq -r ".packages[] | select(.name == \"$crate\") | .manifest_path" | xargs dirname)

        # Get commits since last tag
        if [ -n "$LAST_TAG" ]; then
            commits=$(git log --oneline "$LAST_TAG"..HEAD -- "$crate_path" 2>/dev/null)
        else
            commits=$(git log --oneline -- "$crate_path" 2>/dev/null)
        fi
        count=$(echo "$commits" | grep -c . || echo 0)

        if [ "$count" -eq 0 ]; then
            echo -e "${DIM}$crate $version (no changes)${NC}"
            continue
        fi

        # Run semver-checks
        output=$(cargo semver-checks -p "$crate" 2>&1) || true

        if echo "$output" | grep -q "FAIL"; then
            reason=$(echo "$output" | grep -m1 "FAIL" | sed 's/.*FAIL.*major *//' | awk '{print $1}')
            next=$(bump_version "$version" major)
            echo -e "$crate $version ${RED}<major>${NC} $next"
        elif echo "$output" | grep -q "Summary minor"; then
            next=$(bump_version "$version" minor)
            echo -e "$crate $version ${YELLOW}<minor>${NC} $next"
        else
            next=$(bump_version "$version" patch)
            echo -e "$crate $version ${GREEN}<patch>${NC} $next"
        fi

        # Show commits as tree
        total=$(echo "$commits" | wc -l)
        i=0
        echo "$commits" | while read -r line; do
            i=$((i+1))
            msg=$(echo "$line" | sed 's/^[a-f0-9]* //')
            if [ $i -eq $total ]; then
                echo -e "  ${DIM}└─${NC} $msg"
            else
                echo -e "  ${DIM}├─${NC} $msg"
            fi
        done
    done

# =============================================================================
# Cross-compilation (for releases)
# =============================================================================

# Build for Linux x86_64
build-linux-x64:
    cargo build --release --target x86_64-unknown-linux-gnu --bin fionn

# Build for Linux ARM64
build-linux-arm64:
    cargo build --release --target aarch64-unknown-linux-gnu --bin fionn

# Build for macOS x86_64
build-macos-x64:
    cargo build --release --target x86_64-apple-darwin --bin fionn

# Build for macOS ARM64
build-macos-arm64:
    cargo build --release --target aarch64-apple-darwin --bin fionn

# Build for Windows x86_64
build-windows-x64:
    cargo build --release --target x86_64-pc-windows-msvc --bin fionn

# =============================================================================
# Development Utilities
# =============================================================================

# Install all development dependencies
install-dev-deps:
    cargo install cargo-audit cargo-deny cargo-llvm-cov cargo-semver-checks critcmp maturin

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
