# Validation evidence

This document records evidence for the current unreleased implementation. It is
not a substitute for the manual Zed qualification checklist in
[manual testing](manual-testing.md), and it does not claim registry publication
or end-to-end hosted installation.

## Snapshot

- Date: 2026-10-01
- Hosted CI baseline: `9875ce49de42849f97fcb3ab2b1fed7a162fe1f9`
- Locally validated implementation: `a4f6960a6aa20a6eec996213463c5fac82adcb6b`
- Rust: 1.99.0
- GitHub Actions CI run: [#28](https://github.com/chiploom/zed-wit/actions/runs/36940382890)
- CI conclusion: success

The hosted matrix below applies to the `main` baseline. The local Apple Silicon
validation applies to `a4f6960a6aa20a6eec996213463c5fac82adcb6b`, which includes
the closed-sibling diagnostics fix, corrupt-cache recovery, package-isolated
fixtures, formatter regression fixes, project-local Zed settings, and native
server build-commit reporting. An earlier LSP lifecycle experiment was fully
reverted and has no effective source diff against `main`.

## Hosted CI evidence

Run #28 completed successfully on the baseline commit with the following jobs:

| Scope | Platform / target | Result |
| --- | --- | --- |
| Quality | Ubuntu 24.04 | Passed |
| Native tests | macOS 15 / aarch64-apple-darwin | Passed |
| Native tests | Ubuntu 24.04 / x86_64-unknown-linux-gnu | Passed |
| Native tests | Windows 2025 / x86_64-pc-windows-msvc | Passed |
| Portability check | macOS 15 Intel / x86_64-apple-darwin | Passed |
| Portability check | Ubuntu 24.04 ARM / aarch64-unknown-linux-gnu | Passed |
| Release artifact build | aarch64-apple-darwin | Passed |
| Release artifact build | x86_64-apple-darwin | Passed |
| Release artifact build | aarch64-unknown-linux-gnu | Passed |
| Release artifact build | x86_64-unknown-linux-gnu | Passed |
| Release artifact build | x86_64-pc-windows-msvc | Passed |
| Combined artifact verification | Ubuntu 24.04 | Passed |

The quality job passed:

- `cargo xtask check-no-python`
- `cargo xtask check-dependencies`
- `cargo test -p xtask --locked`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo check --workspace --locked`
- workspace doctests
- `cargo check -p zed-wit --target wasm32-wasip2 --locked`

Each native test job ran the workspace test suite with cargo-nextest for its
native target. Each artifact job built the native language server in release
mode, packaged the executable, and generated redistribution notices. The final
verification job downloaded all five target artifacts together and passed
`cargo xtask verify-release-assets --input dist`.

## Local Apple Silicon evidence

On an `aarch64-apple-darwin` development host using Rust 1.99.0,
`a4f6960a6aa20a6eec996213463c5fac82adcb6b` passed:

- `cargo xtask check-no-python`
- `cargo xtask check-dependencies`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --all-features --locked`
- `cargo check --workspace --locked`
- `cargo build --target wasm32-wasip2 --locked`
- `cargo build -p wit-language-server --release --locked`
- `git diff --check origin/main...HEAD`

The workspace run included 12 `wit-analysis` tests, 3 language-server unit
tests, 4 stdio integration tests, 9 syntax/editing tests, 7 `xtask` tests, and
6 adapter/distribution tests, all passing. The release binary reported
`wit-language-server 0.1.0+git.a4f6960a6aa20a6eec996213463c5fac82adcb6b`,
matching the validated Git `HEAD`.

The `aarch64-apple-darwin` release packaging path also completed successfully
and produced the native executable, SHA-256 checksum, provenance JSON, and
target-specific license report.

## Manual Zed development-extension evidence

The same branch was exercised as a development extension in Zed on Apple
Silicon. Observed behavior included:

- the project-local binary override resolving the repository release server;
- LSP `serverInfo.version` exposing the embedded `+git.<commit>` build identity;
- clean diagnostics for the current and gated package fixtures exercised;
- the intentional `legacy-named-results` grammar gap producing the expected
  `wit-parser` error, `expected a type, found '('`;
- document formatting preserving annotation spacing and upstream
  `use`/`include ... with` spacing while leaving formatted WIT semantically
  clean.

The exact Zed editor version was not recorded in the supplied evidence, so this
is a development-extension smoke/behavior record rather than a complete signed-off
GUI qualification matrix.

## What remains unqualified

The following remain pending and must not be inferred from the green CI matrix:

- A complete manual GUI qualification record with the exact Zed editor version,
  screenshots/log paths, and every checklist row marked with evidence.
- End-to-end automatic native-server download from an actual GitHub release.
- Corrupt/missing hosted asset behavior against a published release.
- Execution of the protected release workflow and artifact attestations.
- Registry succession from the existing `wit` extension.
- Publication eligibility while this repository remains private.

Use [manual testing](manual-testing.md) for the editor qualification procedure.
The repository must be public before a Zed registry submission, and the existing
`wit` registry entry requires the succession process documented in
[publishing](publishing.md).

## Release gate

Before the first release candidate is considered ready:

1. Keep CI green on the exact candidate commit.
2. Complete and record the applicable manual Zed scenarios.
3. Make the repository public before registry submission.
4. Resolve the existing `wit` registry ownership/succession requirement.
5. Create a stable `vX.Y.Z` tag whose version matches the adapter, extension,
   and native server manifests.
6. Dispatch the protected release workflow from the default branch.
7. Verify the published five-target asset set, checksums, provenance, licenses,
   and attestations.
8. Re-test first install and cached install in Zed against the published assets.
