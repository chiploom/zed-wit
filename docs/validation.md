# Validation evidence

This document records evidence for the current unreleased implementation. It is
not a substitute for the manual Zed qualification checklist in
[manual testing](manual-testing.md), and it does not claim registry publication
or end-to-end hosted installation.

## Snapshot

- Date: 2026-10-01
- Baseline commit: `9875ce49de42849f97fcb3ab2b1fed7a162fe1f9`
- Rust: 1.99.0
- GitHub Actions CI run: [#28](https://github.com/chiploom/zed-wit/actions/runs/36940382890)
- CI conclusion: success

The `finish-zed-wit` audit branch restored this exact source tree after an
invalidated LSP lifecycle experiment. Validation claims below therefore refer to
the baseline tree, not to that reverted experiment.

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

On an `aarch64-apple-darwin` development host using Rust 1.99.0, the baseline
tree passed:

- dependency policy validation
- formatting
- clippy with warnings denied
- the full workspace test suite with all features
- workspace checks
- the Zed adapter `wasm32-wasip2` check
- direct `wit-language-server`, `wit-analysis`, and `wit-syntax` test suites

The hosted quality job is the recorded evidence for `check-no-python`.

## What remains unqualified

The following remain pending and must not be inferred from the green CI matrix:

- Manual Zed development-extension installation and editor behavior.
- GUI validation of highlighting, outline, snippets, diagnostics, formatting,
  restart behavior, and log output.
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
