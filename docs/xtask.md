# Rust xtask command reference

Run `cargo xtask <command>` from the workspace. Commands use the pinned Rust toolchain, `Cargo.lock`, and repository-root path resolution. See `cargo xtask help` or `cargo xtask <command> --help`.

Local xtasks **never** create tags, publish GitHub Releases, submit registry updates, or change an upstream grammar pin. Publication remains exclusively in the protected CD workflow.

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
| `dev` | Build optimized native server and validate committed local Zed settings example. Does not write user settings. |
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

Single-target local verification does not replace the existing five-target release asset contract, GitHub attestation, immutable tags, protected `release` environment, or published-download validation. See [publishing](publishing.md). Zed registry succession requires independent authorization.

## Maintenance and optional tooling

| Command | Behavior |
| --- | --- |
| `install-dev --destination <binary-path>` | Copies the already-built native release server into an explicit **new** path; refuses overwrite. |
| `clean --scope <dist|profiles|coverage|build|all>` | Preview by default; `--execute true` deletes only allowed generated outputs. `build`/`all` use Cargo clean. Symlinked target paths are rejected. |
| `bench [--iterations N]` | Run a real optimized Tree-sitter WIT parse microbenchmark with 100 warmups and N timed iterations (default 2,000). |
| `coverage [--output target/coverage/name.lcov]` | Generate LCOV data with optional installed `cargo-llvm-cov`. Output must be a file directly under `target/coverage`. |
| `update-grammar [--candidate <sha>]` | Read-only check of grammar SHA in both manifests and dated compatibility notes; candidate is review guidance only, never an automatic pin update. |

Cleanup examples:

```sh
cargo xtask clean --scope profiles
cargo xtask clean --scope profiles --execute true
cargo xtask clean --scope build --execute true
```

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
