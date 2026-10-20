# Rust xtask command reference

Run `cargo xtask <command>` from the workspace. Commands use the pinned Rust toolchain, `Cargo.lock`, and repository-root path resolution. See `cargo xtask help` or `cargo xtask <command> --help`.

## Code organization

- `crates/xtask/src/main.rs`: entry point, error reporting and exit codes only.
- `crates/xtask/src/cli.rs`: the **single routing match** and complete help registry for all existing and new commands.
- `crates/xtask/src/tasks/build.rs`: builds, local development setup and binary installation.
- `crates/xtask/src/tasks/validation.rs`: test orchestration, compilation checks and quality gates.
- `crates/xtask/src/tasks/release_ops.rs`: local release candidate checks and preparation, **not publication**.
- `crates/xtask/src/tasks/publish.rs`: version-aware planning, explicitly approved release-preparation PRs and requests to existing protected CD.
- `crates/xtask/src/tasks/maintenance.rs`: toolchain diagnostics, scoped cleanup and read-only grammar-pin audits.
- `crates/xtask/src/tasks/performance.rs`: WIT parser benchmarks and optional coverage.
- `crates/xtask/src/tasks/common.rs`: shared process, target and safe-copy utilities.
- `crates/xtask/src/tasks/mod.rs`: module wiring and regression tests.
- Existing `release.rs`, `licenses.rs`, `dependency_policy.rs`, `repository_policy.rs` and `zed_*.rs` retain their specialized implementations.

To add a command: implement it in its domain module, register **one dispatch arm and one help entry** in `cli.rs`, and add argument and failure-path tests. Avoid adding a parallel dispatcher. Version-aware publish actions are tracked in issue #24 and must retain protected CD as the sole publication mechanism.

Local xtasks **never** create tags, publish GitHub Releases, submit registry updates, or change an upstream grammar pin. The new `publish` command can **request** the existing protected CD workflow only after an explicit reviewed PR merge and `--resume --pr N --confirm`; the protected workflow alone performs publication.

Native binary lookup and local release packaging read the effective output directory from `cargo metadata --format-version 1 --no-deps --locked`. This respects `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR`, and Cargo's layered `[build] target-dir` configuration, including relative and absolute paths. The committed Zed settings example requires `target/release/wit-language-server[.exe]`: `dev` checks the actual compiler artifact path and fails if an implicit `build.target` or custom target directory moves the executable. It does not modify Zed settings.

## Development and validation

| Command | Behavior |
| --- | --- |
| `check` | Run workspace rustfmt, all-target/all-feature Clippy, and locked `cargo check`. |
| `test` | Native tests; supports `--package <name>`, `--filter <substring>`, `--runner <auto|cargo|nextest>`. Auto prefers installed nextest unless a substring filter is given. |
| `verify` | Repository policy + metadata-derived dependency checks, `check` and `test-all`. |
| `test-all` | Native workspace tests, doctests and a Wasm extension build; optional `--with-zed true` for real Zed smoke. GUI/hosted tests remain separate. |
| `test-publish` | Offline LSP/extension release lifecycle tests plus the protected-CD asset SHA-256 verifier exercised against mocked GitHub API responses (23 LSP assets and extension zero-assets). Never publishes anything. |
| `test-lsp` | Test syntax, analysis and language-server packages. |
| `test-extension` | Test extension and syntax packages, build `wasm32-wasip2` adapter. |
| `build` | Build `--kind server` (default) or `--kind extension`. Supports `--release true` and server `--target <triple>`. |
| `dev` | Build optimized native server, verify Cargo's actual executable path matches the committed Zed settings example, and leave user settings untouched. |
| `doctor` | Inspect Rust/Cargo/rustup/Git and wasm target; report optional nextest, llvm-cov, and Zed tools separately. |

`test --filter` uses Cargo substring syntax. Do not pair it with `--runner nextest`, which requires different filter expressions. A default test pass does **not** qualify hosted distribution or cross-platform Zed runtime behavior.

## Dependency policy

`cargo xtask check-dependencies` reads the locked, resolved Cargo graph. The manifests and `Cargo.lock` are the source of truth for versions: xtask does not maintain a second list of expected `wit-parser` or `tree-sitter` releases.

The checker requires one resolved semantic-parser version in each of the native language-server and Zed adapter normal/build dependency closures, and one Tree-sitter runtime version across the workspace. It does **not** require the adapter's transitive WIT parser to have the same version as the native semantic parser. The JSON report displays actual resolved versions, not pinned strings from xtask source. A version change still requires API/fixture/compatibility review and passing tests.

The checker also rejects unreviewed dependency sources and license expressions. The permitted `tree-sitter-wit` Git commit is derived from the two matching, 40-character revision pins in `crates/wit-syntax/Cargo.toml` and `extension.toml`; both the repository origin and the resolved Git source must match. The approved release-target matrix, target-filtered native normal/build closure, and `collect-licenses` redistribution notices remain independently enforced. Valid dependency updates must not bypass those checks.

## Local release preparation, not publication

| Command | Behavior |
| --- | --- |
| `release-build` | `--target <triple>` builds an optimized native LSP binary; defaults to current host if supported. Optional `--output <binary-path>` copies to a **new** file. |
| `release-check` | Requires `--scope <lsp|extension> --tag <tag>`. Checks notes, changelog, clean Git checkout, and existing release identity/version rules. |
| `changelog-check` | Checks changelog structure; add both `--scope` and `--tag` to validate scoped release notes. |
| `release` | Requires `--scope` and `--tag`. For LSP, also requires `--target`: build, package, collect licenses, verify one target. For extension, build optimized Wasm. Optional `--output <dir>` for LSP artifacts (default `dist`). |

Example command forms (not claims the example versions are releasable):

```sh
cargo xtask release-build --target aarch64-apple-darwin
cargo xtask release-check --scope lsp --tag v0.1.3
cargo xtask release --scope lsp --tag v0.1.3 --target aarch64-apple-darwin --output dist
cargo xtask release --scope extension --tag v-extension-0.1.0
```

Release packaging creates each binary and sidecar only if its destination does not already exist, including dangling symlinks. An I/O failure can leave an incomplete newly created file; inspect and remove that candidate explicitly before retrying.

Single-target local verification does not replace the existing five-target release asset contract, GitHub attestation, immutable tags, protected `release` environment, or published-download validation. See [publishing](publishing.md). Zed registry succession requires independent authorization.

## Maintenance and optional tooling

| Command | Behavior |
| --- | --- |
| `install-dev --destination <binary-path> [--target <release-target>]` | Build/update the optimized native server for the selected target (default: host), then copy from the **target-specific** release directory into a **new** destination. Refuses overwrite. This is compatible with `release-build`; it does not install the untargeted binary built by `dev`. |
| `clean --scope <dist|profiles|coverage|build|all>` | Preview by default; `--execute true` deletes only allowed generated outputs. `build`/`all` use Cargo clean. Symlinked target paths are rejected. |
| `bench [--iterations N]` | Run a real optimized Tree-sitter WIT parse microbenchmark with 100 warmups and N timed iterations (default 2,000). |
| `coverage [--output target/coverage/name.lcov]` | Generate LCOV data with optional installed `cargo-llvm-cov`. Output must be a file directly under `target/coverage`. |
| `update-grammar [--candidate <sha>]` | Read-only check of grammar SHA in both manifests and dated compatibility notes; candidate is review guidance only, never an automatic pin update. |

The parser benchmark uses a custom Cargo harness (`harness = false`). Cargo automatically supplies `--bench`; the harness handles it alongside `--iterations`. The argument parser is located at `benches/parse/args.rs`, imported by the benchmark and exercised by regular workspace tests. The `wit-syntax` manifest explicitly sets `autobenches = false`, so Cargo does not discover benchmark helper files as separate executable targets.

Cleanup examples:

```sh
cargo xtask clean --scope profiles
cargo xtask clean --scope profiles --execute true
cargo xtask clean --scope build --execute true
```

For `clean --scope build|all`, xtask asks Cargo to resolve its effective target directory, refuses any non-default location (including `CARGO_BUILD_TARGET_DIR` and configured `build.target-dir`), then passes `--target-dir <repository>/target` explicitly to `cargo clean` to prevent redirection. When intentionally cleaning a custom Cargo target directory, run `cargo clean --locked` yourself after checking the configured path.

On Windows, running `cargo clean` from the active `xtask.exe` would attempt to remove that locked executable. For `--scope build` or `--scope all`, invoke `cargo clean --locked` directly instead. The xtask command rejects destructive execution of those scopes on Windows.

No command deletes personal Zed data or modifies release tags. For grammar requalification, see [upstream compatibility](upstream-compatibility.md) and issue #17. Changing a pin requires reviewing tree-sitter queries, generated metadata, WIT fixtures and Cargo.lock.

The issue #24 command verifies **live GitHub protection rules** before
remote writes. **For now, zero required reviewers are permitted** for both
release-preparation PRs and CD publication. Submission still requires a PR
rule, six required CI checks and linear history. Resume additionally
requires the `release` environment to restrict deployment to either
protected branches only or exactly one custom deployment policy that permits
only branch `main`, with no tag patterns or wildcard branches. All policy
pages and GitHub's reported count are validated; missing or unreadable data
fails closed. Protected CD repeats these checks before release-related writes.

The 2026-10-13 qualification confirmed the live environment has an exact
`main` custom deployment branch policy, and this policy is now explicitly
supported. Existing effective `main` ruleset protection remains mandatory.
Zero required reviewers remain temporarily authorized; GitHub-native
approval requirements remain effective if enabled later.

Resume further requires the current `main` SHA to be **identical** to the
reviewed preparation PR's merged commit. Any later commit, even one that
retains the version, is rejected. The globally serialized `cd-release`
workflow is checked for active runs across **all** source SHAs and scopes,
not just the selected candidate, to prevent displacement of pending runs.

For publishing, all six required checks must be **strict**, with source
integration ID `15368` (GitHub Actions) in the effective main-branch rules.
Zero required reviewers remain allowed. The `release` workflow also
requires confirmed immutable-release enforcement via an environment-scoped
Administration-read-only `RELEASE_POLICY_READ_TOKEN`; the standard
`GITHUB_TOKEN` cannot authenticate that API. Any missing/disabled policy
fails before release-related writes, and the published release must report
`immutable=true`. See [publishing](publishing.md) for operator setup.
An authorized operator confirmed immutable releases enabled on 2026-10-13.
The scoped read-only token is provisioned, but its runtime authorization is
not verified until protected CD performs the policy check.

## Version-aware release preparation and protected CD

The opt-in `publish` frontend is separate from the existing local `release`
command. It does **not** create protected tags, publish GitHub Releases, upload
assets, attest releases, or submit Zed registry changes.

```sh
cargo xtask publish --scope lsp
cargo xtask publish --scope extension --bump minor --dry-run
cargo xtask publish --scope lsp --prepare --confirm
cargo xtask publish --scope lsp --submit --confirm
cargo xtask publish --scope lsp --resume --pr 123 --confirm --wait
```

A default call (or `--dry-run`) is read-only and prints the current and proposed
version, tag, protected default-branch revision, affected files, preparation
branch, and next step. It requires a clean local `main` that matches remote
`origin/main`, GitHub CLI authentication and access to remote tag/release history.
If any of these checks fails, it stops. Deleted immutable-release tag history
cannot always be recovered via the API; the existing CD workflow remains the
authoritative final gate and its protections must not be bypassed.

`--prepare --confirm` creates a *local* release-preparation branch from the
current protected main commit. It bumps the chosen scope's manifest(s), updates
`Cargo.lock` via an offline Cargo workspace check, and adds release-note and
changelog **TODO placeholders**. This stage never pushes a branch or creates a
PR. A failed stage may leave local changes; inspect them explicitly. Edit the
TODO content into substantive human-reviewed release notes and changelog entries,
run tests and commit the limited release-preparation files yourself.

`--submit --confirm` checks the dedicated branch, file allowlist, reviewed
notes, locked Cargo metadata, local verification, remote tag history, fresh base
and duplicate PR state. Only then does it push the branch and create a PR to
`main`. It never merges. The submit stage validates the fetch repository **and** every effective push
destination, including Git URL rewrites. It rejects multiple push URLs, pins
the actual push to the single authorized canonical GitHub URL, and uses an
explicit absent-ref lease when creating the branch. A competing branch
creation cannot be advanced implicitly; Git rejects the stale lease.

If GitHub accepts a push but PR creation fails, the same confirmed submit
operation can retry PR creation only when the remote preparation branch still
matches the exact validated local HEAD. Changed remote refs, existing PRs,
and ambiguous prior outcomes require manual review; no force-push is permitted.

`--resume --pr N --confirm` must be invoked after that exact preparation PR
merges to `main`. It checks that the PR's release-preparation commit is on the
current protected default branch and checks the tag, release workflow, and
existing workflow dispatch state. Only then does it request
`.github/workflows/release.yml` with `operation=publish`, `scope`, and
`tag` on `main`. No direct publishing is implemented in xtask. The response
must provide a specific GitHub Actions run ID; ambiguous responses require
manual inspection before any retry.

`--wait` is valid only with confirmed resume. It watches the exact returned
run, reports failure/cancellation, and calls a release *published* only when a
successful run is accompanied by the matching non-draft immutable GitHub Release.
Without `--wait`, the outcome is **requested/pending**, not published.

When `--wait` is enabled, xtask queries only the run ID returned by the
confirmed dispatch. GitHub may briefly return incomplete run metadata; xtask
allows up to six short identity rechecks before failing closed. An explicit
wrong run ID or commit is rejected immediately, and a run is never treated as
published until every identity field and the immutable GitHub Release agree.
Neither confirmed dispatch nor workflow success bypasses the `release`
environment's human approval gate or the five-target LSP release contract.

SemVer uses strict stable `X.Y.Z` components with checked arithmetic. Patch is
default; minor resets patch, major resets minor/patch, including pre-1.0 versions
(`0.1.2 --bump major` gives `1.0.0`). Independent LSP and extension streams
remain separate. No automatic runtime-LSP pin changes occur.

This command is being qualified on [PR #26](https://github.com/chiploom/zed-wit/pull/26).
Treat remote publication as a high-risk operation and review exact scope,
tag, PR and run URL before giving confirmation.

## Isolated publishing lifecycle qualification

Run `cargo xtask test-publish` to execute the dedicated offline connected
LSP and extension publishing scenarios. They prepare scoped manifests,
Cargo.lock, reviewed notes and a changelog in disposable Cargo/Git repositories;
atomically submit a branch to a disposable bare remote; simulate a reviewed
squash merge; check provenance and strict policy gates against synthetic GitHub
responses; and validate mock CD dispatch, run identity, and immutable publication.

No production GitHub credentials, network calls, release tags, or protected CD
dispatches are involved. Tests clean up their temporary fixtures automatically.
The Rust scenarios are also included in `cargo xtask verify` and CI; the
standalone mocked asset verifier is run by `cargo xtask test-publish` and the CI
quality job. These scenarios
exercise real Git operations and the production `submit` PR-creation and
`resume` CD-dispatch helpers through deterministic mocked GitHub requests.
The submission fixture tests atomic branch creation, PR identity verification,
partial-response recovery without re-pushing, duplicate-PR refusal, and
conflicting remote refs. The resume fixture checks protected workflow state,
global run history, exact dispatch response, run identity, polling, and immutable
release finalization. Negative cases cover concurrent CD runs, ambiguous
dispatches, unexpected SHA, stalled runs, mutable releases, and controlled retries.

The asset integrity fixture executes `scripts/release-asset-integrity.sh`, the
same shell code used by protected CD, against 23 synthetic LSP assets and a
stubbed GitHub CLI. It verifies exact SHA-256, asset state, size, name, and
pagination, including incomplete uploads, swapped assets, recovery, and
extension releases with no assets.

**Qualification boundary:** the complete `publish --prepare`/`--submit`/`--resume`
CLI commands are not yet invoked as separate child processes against a
fully simulated GitHub transport, and the actual protected CD workflow,
artifact production, and attestations are not executed. Those require a
separate fully isolated external-transport/CD qualification.

```sh
cargo xtask test-publish
```

## Suggested local validation

```sh
cargo xtask doctor
cargo xtask verify
cargo test -p xtask --locked
cargo xtask bench --iterations 2000
cargo xtask test-zed --timeout-seconds 90
```

Hosted, GUI and non-native OS/architecture validation must be performed and recorded separately.
