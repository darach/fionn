# Release Process

Releases run through GitHub Actions. You check the version locally, then trigger the workflow from the command line.

## Quick Reference

```bash
# 1. Check what version to release
just version-check

# 2. Dry run (validates without releasing)
gh workflow run release.yml -f version=0.3.0 -f dry_run=true

# 3. Release
gh workflow run release.yml -f version=0.3.0
```

## How It Works

### 1. Determine the Version

Run `just version-check` on your local main branch. It will:

- Discover every publishable crate in the workspace automatically
- Compare each crate against the last release tag
- Run `cargo semver-checks` to detect breaking changes, new public API, or patch-only fixes
- Check crates.io for crates that have never been published (these require at least a minor bump)
- Print the suggested version and the exact `gh workflow run` command to use

You can also run `just semver-report` for a per-crate breakdown showing the change level and relevant commits.

### 2. Trigger the Release

```bash
gh workflow run release.yml -f version=0.3.0
```

The `prepare` job in CI will:

1. Validate the version format and confirm the tag does not already exist
2. Discover publishable crates via `cargo metadata`
3. Run `cargo semver-checks --workspace`
4. Enforce semver: a breaking change requires a major bump; a new crate or new public API requires at least a minor bump. If the requested version is too low, the job fails with a clear error.
5. Run `cargo release` to bump every `Cargo.toml`, create the release commit, tag it, and push.

Once the tag lands, the remaining jobs run in sequence:

- **build** -- cross-compiles binaries for Linux (x86_64, x86_64-musl, aarch64), macOS (x86_64, aarch64), and Windows (x86_64)
- **collect-hashes** -- gathers SHA-256 checksums for SLSA provenance
- **release** -- creates the GitHub release with binaries, checksums, and a generated changelog
- **sign** -- signs every artifact with cosign (keyless Sigstore OIDC)
- **publish** -- publishes crates to crates.io in topological dependency order, with retries
- **provenance** -- generates SLSA v1.0 provenance attestation
- **pypi** -- builds and publishes Python wheels to PyPI via maturin

### 3. Dry Run

To validate everything without actually releasing:

```bash
gh workflow run release.yml -f version=0.3.0 -f dry_run=true
```

This runs the `prepare` job (version validation, semver checks, crate discovery) but skips the version bump, tag push, build, and publish steps.

## Crate Discovery

No crate lists are hardcoded anywhere. The workspace is queried at runtime:

```
cargo metadata --no-deps --format-version 1 | python3 ... | tsort
```

This pipeline reads `cargo metadata`, extracts path-based dependencies (excluding dev-dependencies), and feeds the edges into `tsort` for a topological sort. Crates with `publish = []` (like `fionn-py`) are excluded automatically.

Adding a new crate to the workspace requires no changes to the release pipeline. It will be discovered, checked, and published in the right order.

## Other Useful Commands

| Command | Purpose |
|---|---|
| `just version-check` | Suggest the next version based on semver analysis |
| `just semver-report` | Per-crate semver breakdown with commit history |
| `just semver-check` | Run `cargo semver-checks --workspace` |
| `just publish-check` | Show which crates changed since the last release |
| `just publish-dry-run-all` | Dry-run `cargo publish` for every crate in order |
| `just publish-dry-run <crate>` | Dry-run `cargo publish` for one crate |

## Backward Compatibility

Pushing a tag manually (e.g. `git tag v0.3.0 && git push --tags`) still works. The `release.yml` workflow also triggers on `push: tags: v*`. In that case, the `prepare` job is skipped and the pipeline starts directly at `build`. This path does not enforce semver checks.

## Signing and Verification

Every release artifact is signed with [Sigstore](https://www.sigstore.dev/) keyless signing. To verify a downloaded artifact:

```bash
cosign verify-blob fionn-linux-x86_64.tar.gz \
  --signature fionn-linux-x86_64.tar.gz.sig \
  --certificate fionn-linux-x86_64.tar.gz.pem \
  --certificate-identity 'https://github.com/darach/fionn/.github/workflows/release.yml@refs/tags/v0.3.0' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com'
```

SLSA provenance can be verified with:

```bash
slsa-verifier verify-artifact fionn-linux-x86_64.tar.gz \
  --provenance-path multiple.intoto.jsonl \
  --source-uri github.com/darach/fionn \
  --source-tag v0.3.0
```
