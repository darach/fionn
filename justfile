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

# Pre-push dry run: all CI gates without network (no audit)
# Runs: fmt, clippy, test, doc, MSRV, fuzz build check
preflight: fmt-check clippy test doc-check msrv fuzz-check

# Verify fuzz targets compile (separate workspace)
fuzz-check:
    cd fuzz && cargo check

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

    # Discover publishable crates in topological order
    discover_crates() {
      cargo metadata --no-deps --format-version 1 | python3 -c "
    import json, sys
    data = json.load(sys.stdin)
    for pkg in data['packages']:
        if pkg.get('publish') is not None:
            continue
        has_dep = False
        for dep in pkg['dependencies']:
            if dep.get('path') and dep.get('kind') is None:
                has_dep = True
                print(dep['name'] + ' ' + pkg['name'])
        if not has_dep:
            print(pkg['name'] + ' ' + pkg['name'])
    " | tsort
    }

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
    i=1
    for crate in $(discover_crates); do
        echo "  $i. $crate"
        i=$((i + 1))
    done

# Dry-run publish to check for issues
publish-dry-run crate:
    cargo publish -p {{crate}} --dry-run --allow-dirty

# Dry-run publish all crates in dependency order
publish-dry-run-all:
    #!/usr/bin/env bash
    set -e

    # Discover publishable crates in topological order
    discover_crates() {
      cargo metadata --no-deps --format-version 1 | python3 -c "
    import json, sys
    data = json.load(sys.stdin)
    for pkg in data['packages']:
        if pkg.get('publish') is not None:
            continue
        has_dep = False
        for dep in pkg['dependencies']:
            if dep.get('path') and dep.get('kind') is None:
                has_dep = True
                print(dep['name'] + ' ' + pkg['name'])
        if not has_dep:
            print(pkg['name'] + ' ' + pkg['name'])
    " | tsort
    }

    echo "Dry-run publishing all crates in dependency order..."
    for crate in $(discover_crates); do
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

    # Discover publishable crates in topological order
    discover_crates() {
      cargo metadata --no-deps --format-version 1 | python3 -c "
    import json, sys
    data = json.load(sys.stdin)
    for pkg in data['packages']:
        if pkg.get('publish') is not None:
            continue
        has_dep = False
        for dep in pkg['dependencies']:
            if dep.get('path') and dep.get('kind') is None:
                has_dep = True
                print(dep['name'] + ' ' + pkg['name'])
        if not has_dep:
            print(pkg['name'] + ' ' + pkg['name'])
    " | tsort
    }

    CRATES=$(discover_crates)
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

# Suggest next version based on semver compatibility analysis
version-check:
    #!/usr/bin/env bash
    set -euo pipefail

    RED='\033[0;31m'; YELLOW='\033[0;33m'; GREEN='\033[0;32m'
    BOLD='\033[1m'; DIM='\033[0;90m'; NC='\033[0m'

    # Discover publishable crates in topological order
    discover_crates() {
      cargo metadata --no-deps --format-version 1 | python3 -c "
    import json, sys
    data = json.load(sys.stdin)
    for pkg in data['packages']:
        if pkg.get('publish') is not None:
            continue
        has_dep = False
        for dep in pkg['dependencies']:
            if dep.get('path') and dep.get('kind') is None:
                has_dep = True
                print(dep['name'] + ' ' + pkg['name'])
        if not has_dep:
            print(pkg['name'] + ' ' + pkg['name'])
    " | tsort
    }

    CRATES=$(discover_crates)
    LAST_TAG=$(git tag --list 'v*' --sort=-version:refname | head -1)
    CURRENT_VERSION=$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c "
    import json, sys
    pkgs = json.load(sys.stdin)['packages']
    print(next(p['version'] for p in pkgs if p['name'] == 'fionn'))
    ")

    if [ -z "$LAST_TAG" ]; then
        echo "No previous release tags found."
        echo "Suggested version: $CURRENT_VERSION (initial release)"
        exit 0
    fi

    echo -e "${BOLD}Version Check${NC}"
    echo "  current version: $CURRENT_VERSION"
    echo "  last release:    $LAST_TAG"
    echo ""

    bump_version() {
        local ver=$1 level=$2
        IFS='.' read -r major minor patch <<< "$ver"
        case $level in
            major) echo "$((major+1)).0.0" ;;
            minor) echo "$major.$((minor+1)).0" ;;
            patch) echo "$major.$minor.$((patch+1))" ;;
        esac
    }

    # Track the highest required bump across all crates
    MAX_BUMP="patch"
    BREAKING_CRATES=""
    FEATURE_CRATES=""
    CHANGED_CRATES=""
    NEW_CRATES=""
    UNCHANGED=0

    for crate in $CRATES; do
        crate_path=$(cargo metadata --no-deps --format-version 1 2>/dev/null | \
            python3 -c "
    import json, sys, os
    pkgs = json.load(sys.stdin)['packages']
    pkg = next(p for p in pkgs if p['name'] == '$crate')
    print(os.path.dirname(pkg['manifest_path']))
    ")

        # Detect new crates (not yet on crates.io)
        if ! curl -sf "https://crates.io/api/v1/crates/$crate" > /dev/null 2>&1; then
            NEW_CRATES="$NEW_CRATES $crate"
        fi

        # Check for changes since last tag
        commits=$(git log --oneline "$LAST_TAG"..HEAD -- "$crate_path" 2>/dev/null || true)
        count=$(echo "$commits" | grep -c . 2>/dev/null || echo 0)

        if [ "$count" -eq 0 ]; then
            UNCHANGED=$((UNCHANGED + 1))
            continue
        fi

        CHANGED_CRATES="$CHANGED_CRATES $crate"

        # Run semver-checks
        output=$(cargo semver-checks -p "$crate" 2>&1) || true

        if echo "$output" | grep -q "FAIL"; then
            MAX_BUMP="major"
            BREAKING_CRATES="$BREAKING_CRATES $crate"
            echo -e "  ${RED}BREAKING${NC}  $crate"
            # Show the specific breaking changes
            echo "$output" | grep -E "^--- " | head -5 | while read -r line; do
                echo -e "    ${DIM}$line${NC}"
            done
        elif echo "$output" | grep -qE "(Summary.*minor|new pub)"; then
            if [ "$MAX_BUMP" != "major" ]; then
                MAX_BUMP="minor"
            fi
            FEATURE_CRATES="$FEATURE_CRATES $crate"
            echo -e "  ${YELLOW}feature${NC}   $crate"
        else
            echo -e "  ${GREEN}patch${NC}     $crate"
        fi
    done

    # Escalate to minor if there are new crates
    if [ -n "$NEW_CRATES" ] && [ "$MAX_BUMP" = "patch" ]; then
        MAX_BUMP="minor"
        echo -e "  ${YELLOW}new${NC}       (new crates:$NEW_CRATES)"
    fi

    if [ -z "$CHANGED_CRATES" ]; then
        echo "  No crates have changes since $LAST_TAG."
        echo ""
        echo "Nothing to release."
        exit 0
    fi

    echo -e "  ${DIM}($UNCHANGED crate(s) unchanged)${NC}"
    echo ""

    # Determine suggested version
    SUGGESTED=$(bump_version "$CURRENT_VERSION" "$MAX_BUMP")

    # Show summary
    case $MAX_BUMP in
        major)
            echo -e "${BOLD}Suggested version: ${RED}$SUGGESTED${NC} (major — breaking API changes)"
            echo -e "  Breaking crates:$RED$BREAKING_CRATES${NC}"
            ;;
        minor)
            echo -e "${BOLD}Suggested version: ${YELLOW}$SUGGESTED${NC} (minor — new features, backward compatible)"
            echo -e "  Feature crates:$YELLOW$FEATURE_CRATES${NC}"
            if [ -n "$NEW_CRATES" ]; then
                echo -e "  New crates:$YELLOW$NEW_CRATES${NC}"
            fi
            ;;
        patch)
            echo -e "${BOLD}Suggested version: ${GREEN}$SUGGESTED${NC} (patch — bug fixes only)"
            ;;
    esac

    echo ""
    echo -e "To release: ${BOLD}gh workflow run release.yml -f version=$SUGGESTED${NC}"
    echo -e "Dry run:    ${BOLD}gh workflow run release.yml -f version=$SUGGESTED -f dry_run=true${NC}"

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
# Release Signing (requires cosign: https://docs.sigstore.dev/cosign/system_config/installation/)
# =============================================================================

# Test sign+verify cycle: build a release binary, package, sign, and verify
sign-test:
    #!/usr/bin/env bash
    set -euo pipefail

    # Check for cosign
    if ! command -v cosign &>/dev/null; then
        echo "ERROR: cosign not found. Install from https://docs.sigstore.dev/cosign/system_config/installation/"
        exit 1
    fi
    echo "cosign $(cosign version 2>&1 | head -1)"

    WORKDIR=$(mktemp -d)
    trap 'rm -rf "$WORKDIR"' EXIT

    echo "=== Building release binary ==="
    cargo build --release --bin fionn

    echo "=== Packaging test artifact ==="
    tar -czf "$WORKDIR/fionn-test.tar.gz" -C target/release fionn
    sha256sum "$WORKDIR/fionn-test.tar.gz" > "$WORKDIR/fionn-test.tar.gz.sha256"
    ls -lh "$WORKDIR/fionn-test.tar.gz"

    echo "=== Signing (keyless OIDC — opens browser) ==="
    cosign sign-blob --yes "$WORKDIR/fionn-test.tar.gz" \
        --output-signature "$WORKDIR/fionn-test.tar.gz.sig" \
        --output-certificate "$WORKDIR/fionn-test.tar.gz.pem"

    echo "=== Verifying signature ==="
    IDENTITY=$(openssl x509 -in "$WORKDIR/fionn-test.tar.gz.pem" -noout -ext subjectAltName 2>/dev/null \
        | grep -oP 'email:\K[^,]+' || true)
    ISSUER=$(openssl x509 -in "$WORKDIR/fionn-test.tar.gz.pem" -noout -text 2>/dev/null \
        | grep -oP '1.3.6.1.4.1.57264.1.1:\K.*' || true)

    if [ -z "$IDENTITY" ] || [ -z "$ISSUER" ]; then
        echo "Extracting identity from certificate..."
        openssl x509 -in "$WORKDIR/fionn-test.tar.gz.pem" -noout -text | grep -A1 "Subject Alternative Name"
        echo ""
        echo "Enter the email/URI shown above as certificate-identity:"
        read -r IDENTITY
        echo "Enter the OIDC issuer (e.g. https://accounts.google.com, https://github.com/login/oauth, https://token.actions.githubusercontent.com):"
        read -r ISSUER
    fi

    cosign verify-blob "$WORKDIR/fionn-test.tar.gz" \
        --signature "$WORKDIR/fionn-test.tar.gz.sig" \
        --certificate "$WORKDIR/fionn-test.tar.gz.pem" \
        --certificate-identity "$IDENTITY" \
        --certificate-oidc-issuer "$ISSUER"

    echo ""
    echo "=== PASS: sign + verify succeeded ==="
    echo "  artifact:    fionn-test.tar.gz"
    echo "  signature:   fionn-test.tar.gz.sig"
    echo "  certificate: fionn-test.tar.gz.pem"
    echo "  identity:    $IDENTITY"
    echo "  issuer:      $ISSUER"

# Verify a release artifact (usage: just sign-verify fionn-linux-x86_64.tar.gz identity issuer)
sign-verify artifact identity issuer:
    #!/usr/bin/env bash
    set -euo pipefail

    ARTIFACT="{{artifact}}"
    SIG="${ARTIFACT}.sig"
    CERT="${ARTIFACT}.pem"

    for f in "$ARTIFACT" "$SIG" "$CERT"; do
        if [ ! -f "$f" ]; then
            echo "ERROR: $f not found"
            echo "Expected files: $ARTIFACT, $SIG, $CERT"
            exit 1
        fi
    done

    echo "=== Verifying $ARTIFACT ==="
    cosign verify-blob "$ARTIFACT" \
        --signature "$SIG" \
        --certificate "$CERT" \
        --certificate-identity "{{identity}}" \
        --certificate-oidc-issuer "{{issuer}}"

    echo "=== PASS: $ARTIFACT signature valid ==="

# Download and verify a GitHub release artifact (usage: just sign-verify-release v0.1.0 fionn-linux-x86_64.tar.gz identity issuer)
sign-verify-release tag artifact identity issuer:
    #!/usr/bin/env bash
    set -euo pipefail

    WORKDIR=$(mktemp -d)
    trap 'rm -rf "$WORKDIR"' EXIT
    REPO="darach/fionn"

    echo "=== Downloading {{artifact}} from release {{tag}} ==="
    for f in "{{artifact}}" "{{artifact}}.sig" "{{artifact}}.pem" "{{artifact}}.sha256"; do
        echo "  $f"
        gh release download "{{tag}}" --repo "$REPO" --pattern "$f" --dir "$WORKDIR" || \
            echo "  WARN: $f not found in release"
    done

    echo "=== Checking SHA-256 ==="
    if [ -f "$WORKDIR/{{artifact}}.sha256" ]; then
        (cd "$WORKDIR" && sha256sum -c "{{artifact}}.sha256")
    else
        echo "  SKIP: no .sha256 file"
    fi

    echo "=== Verifying cosign signature ==="
    cosign verify-blob "$WORKDIR/{{artifact}}" \
        --signature "$WORKDIR/{{artifact}}.sig" \
        --certificate "$WORKDIR/{{artifact}}.pem" \
        --certificate-identity "{{identity}}" \
        --certificate-oidc-issuer "{{issuer}}"

    echo "=== PASS: {{artifact}} from {{tag}} is valid ==="

# Generate signing readiness report for local build artifacts
sign-report:
    #!/usr/bin/env bash
    set -euo pipefail

    # --- header ---
    echo "==============================================================================="
    echo "  Release Signing Report"
    echo "  $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo "  commit: $(git rev-parse --short HEAD) ($(git rev-parse --abbrev-ref HEAD))"
    echo "==============================================================================="
    echo ""

    # --- tooling ---
    echo "Tooling"
    echo "-------"
    COSIGN_OK=false
    if command -v cosign &>/dev/null; then
        COSIGN_VER=$(cosign version 2>&1 | grep -oP 'v[\d.]+' | head -1 || cosign version 2>&1 | head -1)
        echo "  cosign:  $COSIGN_VER"
        COSIGN_OK=true
    else
        echo "  cosign:  NOT INSTALLED"
    fi
    if command -v openssl &>/dev/null; then
        echo "  openssl: $(openssl version 2>&1 | head -1)"
    else
        echo "  openssl: NOT INSTALLED"
    fi
    echo "  cargo:   $(cargo --version)"
    echo ""

    # --- CI workflow analysis ---
    echo "CI Workflow: .github/workflows/release.yml"
    echo "--------------------------------------------"
    RELEASE_YML=".github/workflows/release.yml"
    if [ -f "$RELEASE_YML" ]; then
        # Check for sign job
        if grep -q "^  sign:" "$RELEASE_YML"; then
            echo "  sign job:         PRESENT"
        else
            echo "  sign job:         MISSING"
        fi

        # Check cosign-installer is pinned
        COSIGN_REF=$(grep -oP 'sigstore/cosign-installer@\S+' "$RELEASE_YML" | head -1 || true)
        if [ -n "$COSIGN_REF" ]; then
            if echo "$COSIGN_REF" | grep -qP '@[0-9a-f]{40}'; then
                echo "  cosign-installer: SHA-pinned ($COSIGN_REF)"
            else
                echo "  cosign-installer: UNPINNED ($COSIGN_REF)"
            fi
        else
            echo "  cosign-installer: NOT FOUND"
        fi

        # Check id-token permission
        if grep -B5 "sign:" "$RELEASE_YML" | grep -q "id-token:" 2>/dev/null || \
           grep -A10 "^  sign:" "$RELEASE_YML" | grep -q "id-token: write" 2>/dev/null; then
            echo "  id-token: write:  PRESENT (keyless OIDC)"
        else
            echo "  id-token: write:  MISSING (keyless signing will fail)"
        fi

        # Check contents: write for uploading sigs
        if grep -A10 "^  sign:" "$RELEASE_YML" | grep -q "contents: write" 2>/dev/null; then
            echo "  contents: write:  PRESENT (can upload to release)"
        else
            echo "  contents: write:  MISSING (cannot upload signatures)"
        fi

        # Check SLSA provenance
        if grep -q "slsa-github-generator" "$RELEASE_YML"; then
            SLSA_REF=$(grep -oP 'slsa-framework/slsa-github-generator/\S+' "$RELEASE_YML" | head -1 || true)
            echo "  SLSA provenance:  PRESENT ($SLSA_REF)"
        else
            echo "  SLSA provenance:  NOT CONFIGURED"
        fi
    else
        echo "  ERROR: $RELEASE_YML not found"
    fi
    echo ""

    # --- build local artifact ---
    echo "Local Artifact Build"
    echo "--------------------"
    WORKDIR=$(mktemp -d)
    trap 'rm -rf "$WORKDIR"' EXIT

    echo "  building release binary..."
    cargo build --release --bin fionn 2>&1 | tail -1

    # Produce all the artifacts the CI matrix would produce (for the local target)
    ARCH=$(uname -m)
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    case "$ARCH" in
        x86_64)  TARGET_ARCH="x86_64" ;;
        aarch64) TARGET_ARCH="aarch64" ;;
        arm64)   TARGET_ARCH="aarch64" ;;
        *)       TARGET_ARCH="$ARCH" ;;
    esac
    ARTIFACT_NAME="fionn-${OS}-${TARGET_ARCH}"

    tar -czf "$WORKDIR/${ARTIFACT_NAME}.tar.gz" -C target/release fionn
    sha256sum "$WORKDIR/${ARTIFACT_NAME}.tar.gz" > "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sha256"

    ARTIFACT_SIZE=$(stat --printf='%s' "$WORKDIR/${ARTIFACT_NAME}.tar.gz" 2>/dev/null || stat -f%z "$WORKDIR/${ARTIFACT_NAME}.tar.gz")
    ARTIFACT_SHA=$(cut -d' ' -f1 "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sha256")
    echo "  artifact: ${ARTIFACT_NAME}.tar.gz ($(numfmt --to=iec "$ARTIFACT_SIZE" 2>/dev/null || echo "${ARTIFACT_SIZE} bytes"))"
    echo "  sha256:   ${ARTIFACT_SHA}"
    echo ""

    # --- CI release matrix ---
    echo "Release Matrix (CI)"
    echo "--------------------"
    echo "  The release workflow builds these artifacts:"
    echo ""
    printf "  %-34s  %-16s  %s\n" "ARTIFACT" "RUNNER" "STATUS"
    printf "  %-34s  %-16s  %s\n" "--------" "------" "------"

    # These match the matrix in release.yml
    TARGETS=(
        "fionn-linux-x86_64.tar.gz|ubuntu-latest|x86_64-unknown-linux-gnu"
        "fionn-linux-x86_64-musl.tar.gz|ubuntu-latest|x86_64-unknown-linux-musl"
        "fionn-linux-aarch64.tar.gz|ubuntu-latest|aarch64-unknown-linux-gnu"
        "fionn-macos-x86_64.tar.gz|macos-latest|x86_64-apple-darwin"
        "fionn-macos-aarch64.tar.gz|macos-latest|aarch64-apple-darwin"
        "fionn-windows-x86_64.zip|windows-latest|x86_64-pc-windows-msvc"
    )

    for entry in "${TARGETS[@]}"; do
        IFS='|' read -r name runner target <<< "$entry"
        if [ "$name" = "${ARTIFACT_NAME}.tar.gz" ]; then
            printf "  %-34s  %-16s  built (local)\n" "$name" "$runner"
        else
            printf "  %-34s  %-16s  CI only\n" "$name" "$runner"
        fi
    done
    echo ""

    # --- signing check ---
    echo "Signing Check"
    echo "-------------"
    SIGN_OK=false
    VERIFY_OK=false

    EXPECTED_SIGS=(
        "fionn-linux-x86_64.tar.gz.sig"
        "fionn-linux-x86_64.tar.gz.pem"
        "fionn-linux-x86_64-musl.tar.gz.sig"
        "fionn-linux-x86_64-musl.tar.gz.pem"
        "fionn-linux-aarch64.tar.gz.sig"
        "fionn-linux-aarch64.tar.gz.pem"
        "fionn-macos-x86_64.tar.gz.sig"
        "fionn-macos-x86_64.tar.gz.pem"
        "fionn-macos-aarch64.tar.gz.sig"
        "fionn-macos-aarch64.tar.gz.pem"
        "fionn-windows-x86_64.zip.sig"
        "fionn-windows-x86_64.zip.pem"
    )

    if [ "$COSIGN_OK" = true ]; then
        # Local key-pair sign+verify (no network, no OIDC)
        echo "  generating ephemeral test keypair..."
        COSIGN_PASSWORD="" cosign generate-key-pair --output-key-prefix "$WORKDIR/test" 2>/dev/null
        echo ""

        echo "  signing local artifact with test key..."
        if COSIGN_PASSWORD="" cosign sign-blob --yes --tlog-upload=false --key "$WORKDIR/test.key" \
            --output-signature "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sig" \
            "$WORKDIR/${ARTIFACT_NAME}.tar.gz" 2>&1; then
            SIGN_OK=true
            SIG_SIZE=$(stat --printf='%s' "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sig" 2>/dev/null || stat -f%z "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sig")
            echo "  signature:   ${ARTIFACT_NAME}.tar.gz.sig (${SIG_SIZE} bytes)"
        else
            echo "  signing failed"
        fi
        echo ""

        if [ "$SIGN_OK" = true ]; then
            echo "  verifying signature with test public key..."
            if cosign verify-blob --insecure-ignore-tlog --key "$WORKDIR/test.pub" \
                --signature "$WORKDIR/${ARTIFACT_NAME}.tar.gz.sig" \
                "$WORKDIR/${ARTIFACT_NAME}.tar.gz" 2>&1; then
                VERIFY_OK=true
                echo "    Result: PASS"
            else
                echo "    Result: FAIL"
            fi
        fi
        echo ""
        echo "  note: CI uses keyless Sigstore OIDC (id-token: write), not a static key."
        echo "  run 'just sign-test' interactively to test the full keyless flow."
    else
        echo "  SKIP: cosign not installed — cannot test signing"
        echo "  Install: https://docs.sigstore.dev/cosign/system_config/installation/"
    fi
    echo ""

    # --- per-artifact expected files table ---
    echo "Expected Release Assets (per artifact)"
    echo "---------------------------------------"
    printf "  %-38s  %s\n" "FILE" "PURPOSE"
    printf "  %-38s  %s\n" "----" "-------"
    printf "  %-38s  %s\n" "<name>.tar.gz / .zip"       "release binary"
    printf "  %-38s  %s\n" "<name>.tar.gz.sha256"        "SHA-256 checksum"
    printf "  %-38s  %s\n" "<name>.tar.gz.sig"           "cosign detached signature"
    printf "  %-38s  %s\n" "<name>.tar.gz.pem"           "Fulcio signing certificate"
    printf "  %-38s  %s\n" "multiple.intoto.jsonl"       "SLSA provenance attestation"
    echo ""

    # --- CI workflow action pins ---
    echo "Action Pin Audit (release.yml)"
    echo "------------------------------"
    UNPINNED=0
    while IFS= read -r line; do
        ref=$(echo "$line" | grep -oP 'uses: \K\S+')
        if echo "$ref" | grep -qP '@[0-9a-f]{40}'; then
            printf "  PINNED   %s\n" "$ref"
        else
            printf "  UNPINNED %s\n" "$ref"
            UNPINNED=$((UNPINNED + 1))
        fi
    done < <(grep -E '^\s+uses:' "$RELEASE_YML")
    if [ "$UNPINNED" -eq 0 ]; then
        echo "  All actions are SHA-pinned."
    else
        echo "  WARNING: $UNPINNED action(s) not SHA-pinned."
    fi
    echo ""

    # --- verification commands for users ---
    echo "Verification Commands (for end users)"
    echo "--------------------------------------"
    echo "  # After a release, users can verify with:"
    echo ""
    echo "  # 1. Check SHA-256"
    echo "  sha256sum -c fionn-linux-x86_64.tar.gz.sha256"
    echo ""
    echo "  # 2. Verify cosign signature (CI identity)"
    echo "  cosign verify-blob fionn-linux-x86_64.tar.gz \\"
    echo "    --signature fionn-linux-x86_64.tar.gz.sig \\"
    echo "    --certificate fionn-linux-x86_64.tar.gz.pem \\"
    echo "    --certificate-identity 'https://github.com/darach/fionn/.github/workflows/release.yml@refs/tags/<TAG>' \\"
    echo "    --certificate-oidc-issuer 'https://token.actions.githubusercontent.com'"
    echo ""
    echo "  # 3. Verify SLSA provenance"
    echo "  slsa-verifier verify-artifact fionn-linux-x86_64.tar.gz \\"
    echo "    --provenance-path multiple.intoto.jsonl \\"
    echo "    --source-uri github.com/darach/fionn \\"
    echo "    --source-tag <TAG>"
    echo ""

    # --- summary ---
    echo "==============================================================================="
    echo "  Summary"
    echo "==============================================================================="
    CHECKS_PASS=0
    CHECKS_FAIL=0

    check() {
        local label="$1" ok="$2"
        if [ "$ok" = "true" ]; then
            printf "  [PASS]  %s\n" "$label"
            CHECKS_PASS=$((CHECKS_PASS + 1))
        else
            printf "  [FAIL]  %s\n" "$label"
            CHECKS_FAIL=$((CHECKS_FAIL + 1))
        fi
    }

    check "sign job in release.yml" "$(grep -q '^  sign:' "$RELEASE_YML" && echo true || echo false)"
    check "cosign-installer SHA-pinned" "$(grep -P 'cosign-installer@[0-9a-f]{40}' "$RELEASE_YML" >/dev/null && echo true || echo false)"
    check "id-token: write permission" "$(grep -A10 '^  sign:' "$RELEASE_YML" | grep -q 'id-token: write' && echo true || echo false)"
    check "contents: write permission" "$(grep -A10 '^  sign:' "$RELEASE_YML" | grep -q 'contents: write' && echo true || echo false)"
    check "SLSA provenance configured" "$(grep -q 'slsa-github-generator' "$RELEASE_YML" && echo true || echo false)"
    check "all release.yml actions SHA-pinned" "$([ $UNPINNED -eq 0 ] && echo true || echo false)"
    check "cosign installed locally" "$COSIGN_OK"
    check "local artifact signed" "$SIGN_OK"
    check "local signature verified" "$VERIFY_OK"

    echo ""
    echo "  $CHECKS_PASS passed, $CHECKS_FAIL failed"
    echo "==============================================================================="

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
