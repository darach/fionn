#!/bin/bash
# AFL Fuzz testing script for fionn
#
# Prerequisites:
#   cargo install cargo-afl
#   (on Ubuntu/Debian: apt-get install afl++)
#
# Usage:
#   ./fuzz/scripts/fuzz.sh [target] [duration]
#
# Examples:
#   ./fuzz/scripts/fuzz.sh path          # Fuzz path parsing indefinitely
#   ./fuzz/scripts/fuzz.sh jsonl 1h      # Fuzz JSONL for 1 hour
#   ./fuzz/scripts/fuzz.sh all 30m       # Fuzz all targets for 30 min each
#   ./fuzz/scripts/fuzz.sh build         # Just build fuzz targets

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Ensure we're in project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_ROOT"

FUZZ_DIR="$PROJECT_ROOT/fuzz"
OUTPUT_DIR="$FUZZ_DIR/output"
CORPUS_DIR="$FUZZ_DIR/corpus"

# Parse arguments
TARGET="${1:-all}"
DURATION="${2:-}"

# Check prerequisites
check_prerequisites() {
    echo -e "${BLUE}Checking prerequisites...${NC}"

    if ! command -v cargo-afl &> /dev/null; then
        echo -e "${RED}Error: cargo-afl not found${NC}"
        echo ""
        echo "Install with:"
        echo "  cargo install cargo-afl"
        echo ""
        exit 1
    fi

    if ! command -v afl-fuzz &> /dev/null; then
        echo -e "${YELLOW}Warning: afl-fuzz not in PATH${NC}"
        echo "AFL++ may not be installed system-wide."
        echo "cargo-afl will use its bundled version."
    fi

    echo -e "${GREEN}Prerequisites OK${NC}"
}

# Build fuzz targets
build_targets() {
    echo -e "${BLUE}Building fuzz targets with AFL instrumentation...${NC}"

    # Build all fuzz targets with afl-fuzz feature
    cargo afl build --release --features afl-fuzz --bin fuzz_path
    cargo afl build --release --features afl-fuzz --bin fuzz_jsonl
    cargo afl build --release --features afl-fuzz --bin fuzz_classify
    cargo afl build --release --features afl-fuzz --bin fuzz_tape

    echo -e "${GREEN}Build complete${NC}"
}

# Run fuzzer for a specific target
run_fuzzer() {
    local target=$1
    local corpus="$CORPUS_DIR/$target"
    local output="$OUTPUT_DIR/$target"
    local binary="$PROJECT_ROOT/target/release/fuzz_$target"

    echo -e "${YELLOW}Fuzzing: $target${NC}"
    echo "  Corpus: $corpus"
    echo "  Output: $output"
    echo "  Binary: $binary"

    # Create output directory
    mkdir -p "$output"

    # Build duration argument
    local duration_arg=""
    if [ -n "$DURATION" ]; then
        duration_arg="-V $DURATION"
    fi

    # Set AFL environment
    export AFL_SKIP_CPUFREQ=1
    export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1

    # Run AFL
    echo ""
    echo -e "${BLUE}Starting AFL...${NC}"
    echo "Press Ctrl+C to stop"
    echo ""

    # shellcheck disable=SC2086
    cargo afl fuzz \
        -i "$corpus" \
        -o "$output" \
        $duration_arg \
        "$binary"
}

# Run all fuzzers sequentially
run_all() {
    local targets=("path" "jsonl" "classify" "tape")

    for target in "${targets[@]}"; do
        echo ""
        echo -e "${BLUE}========================================${NC}"
        run_fuzzer "$target"
        echo -e "${BLUE}========================================${NC}"
    done
}

# Main
main() {
    echo -e "${BLUE}=== fionn AFL Fuzz Testing ===${NC}"
    echo ""

    check_prerequisites

    case "$TARGET" in
        "build")
            build_targets
            ;;
        "path"|"jsonl"|"classify"|"tape")
            build_targets
            run_fuzzer "$TARGET"
            ;;
        "all")
            build_targets
            run_all
            ;;
        *)
            echo "Usage: $0 [target] [duration]"
            echo ""
            echo "Targets:"
            echo "  build    - Build fuzz targets only"
            echo "  path     - Fuzz path parsing"
            echo "  jsonl    - Fuzz JSONL processing"
            echo "  classify - Fuzz character classification"
            echo "  tape     - Fuzz tape parsing"
            echo "  all      - Fuzz all targets"
            echo ""
            echo "Duration examples: 30s, 5m, 1h, 24h"
            exit 1
            ;;
    esac
}

main
