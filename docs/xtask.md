# Rust xtask command reference

Run `cargo xtask <command>` from the workspace. Commands use the pinned Rust toolchain, `Cargo.lock`, and repository-root path resolution. See `cargo xtask help` or `cargo xtask <command> --help`.

## Code organization

- `crates/xtask/src/main.rs`: entry point, error reporting and exit codes only.
- `crates/xtask/src/cli.rs`: the **single routing match** and complete help registry for all existing and new commands.
- `crates/xtask/src/tasks/build.rs`: builds, local development setup and binary installation.
- `crates/xtask/src/tasks/validation.rs`: test orchestration, compilation checks and quality gates.
- `crates/xtask/src/tasks/release_ops.rs`: local release candidate checks and preparation, **not publication**.
- `crates/xtask/src/tasks/maintenance.rs`: toolchain diagnostics, scoped cleanup and read-only grammar-pin audits.
- `crates/xtask/src/tasks/performance.rs`: WIT parser benchmarks and optional coverage.
- `crates/xtask/src/tasks/common.rs`: shared process, target and safe-copy utilities.
- `crates/xtask/src/tasks/mod.rs`: module wiring and regression tests.
- Existing `release.rs`, `licenses.rs`, `dependency_policy.rs`, `repository_policy.rs` and `zed_*.rs` retain their specialized implementations.

To add a command: implement it in its domain module, register **one dispatch arm and one help entry** in `cli.rs`, and add argument and failure-path tests. Avoid adding a parallel dispatcher. See separate issue #24 for a future version-aware publish workflow.

Local xtasks **never** create tags, publish GitHub Releases, submit registry updates, or change an upstream grammar pin. Publication remains exclusively in the protected CD workflow.

Native binary lookup and local release packaging read the effective output directory from `cargo metadata --format-version 1 --no-deps --locked`. This respects `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR`, and Cargo's layered `[build] target-dir` configuration, including relative and absolute paths. The committed Zed settings example requires `target/release/wit-language-server[.exe]`: `dev` checks the actual compiler artifact path and fails if an implicit `build.target` or custom target directory moves the executable. It does not modify Zed settings.

## Development and validation

| Command | Behavior |
| --- | --- |
| `check` | Run workspace rustfmt, all-target/all-feature Clippy, and locked `cargo check`. |
| `test` | Native tests; supports `--package <name>`, `--filter <substring>`, `--runner <auto|cargo|nextest>`. Auto prefers installed nextest unless a substring filter is given. |
| `verify` | Repository policy + dependency checks, `check` and `test-all`. |
| `test-all` | Native workspace tests, doctests and a Wasm extension build; optional `--with-zed true` for real Zed smoke. GUI/hosted tests remain separate. |
| `test-lsp` | Test syntax, analysis and language-server packages. |
| `test-extension` | Test extension and syntax packages, build `wasm32-wasip2` adapter. |
| `build` | Build `--kind server` (default) or `--kind extension`. Supports `--release true` and server `--target <triple>`. |
| `dev` | Build optimized native server, verify Cargo's actual executable path matches the committed Zed settings example, and leave user settings untouched. |
| `doctor` | Inspect Rust/Cargo/rustup/Git and wasm target; report optional nextest, llvm-cov, and Zed tools separately. |

`test --filter` uses Cargo substring syntax. Do not pair it with `--runner nextest`, which requires different filter expressions. A default test pass does **not** qualify hosted distribution or cross-platform Zed runtime behavior.

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

## Suggested local validation

```sh
cargo xtask doctor
cargo xtask verify
cargo test -p xtask --locked
cargo xtask bench --iterations 2000
cargo xtask test-zed --timeout-seconds 90
```

Hosted, GUI and non-native OS/architecture validation must be performed and recorded separately.
