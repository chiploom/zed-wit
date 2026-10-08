# Validation evidence

This document records qualification evidence for the implementation and its
published native release. It is not a substitute for the reusable Zed
qualification checklist in [manual testing](manual-testing.md), and it does not
claim Zed registry publication.

## v0.1.2 LSP publication and adapter qualification — 2026-10-08

The [immutable `v0.1.2` release](https://github.com/chiploom/zed-wit/releases/tag/v0.1.2),
published on 2026-10-07, is non-draft and non-prerelease. Its protected tag
resolves to `3d067f3ea7dffdd4d091d2d5d62be8ac025ada5b`. The release
contains five native server binaries, their respective SHA-256 checksums,
provenance records and license bundles, and the project licenses and
third-party notices.

### Adapter qualification — macOS ARM64

[PR #21](https://github.com/chiploom/zed-wit/pull/21) records local
qualification on 2026-10-08 using macOS ARM64 and Zed 1.23.2, at adapter
commit `6fcce89cecbe9fb8c58cf598507cebf439abe235`. The adapter pins
`package.metadata.zed-wit.runtime-lsp-version` to `0.1.2` independently
of its own package version.

The recorded validation covered:

- download and SHA-256 sidecar verification for all five published binaries;
- `cargo xtask test-zed` for native server startup/restart and semantic fixtures;
- `cargo xtask test-zed-hosted` for first download, verified cache reuse,
  corrupt-binary recovery, and missing-checksum recovery using the published
  `v0.1.2` native server; the released macOS ARM64 binary has SHA-256
  `6b4f46128fca48e089b5ab70534ea3aa2a1ce4488ba07520587712d6088c0c5f`;
- `cargo xtask test-zed-gui --allow-input-injection true` for real-Zed
  snippet insertion/tab stops and outline navigation.

The hosted and GUI commands record their results respectively in
`target/zed-hosted/profile/zed-hosted-report.json` and
`target/zed-gui/zed-gui-report.json` in the local qualification workspace.

These results were reported from the local validation environment in PR #21,
not independently reproduced by this documentation update. The subsequent
changes to the PR update documentation only.

Real-Zed hosted delivery on macOS x86_64, Linux GNU ARM64/x86_64, and
Windows x86_64 MSVC is not yet qualified; it is tracked in
[issue #22](https://github.com/chiploom/zed-wit/issues/22). Native
cross-platform CI and release-asset checksum verification are separate
evidence and do not establish real-Zed runtime qualification.

## v0.1.1 LSP recovery publication — 2026-10-07

The immutable `v0.1.1` GitHub Release restores native LSP distribution after
the deleted immutable `v0.1.0` release. The protected `v0.1.1` tag resolves
directly to
`4019fcc2b63c26702ad7d77839f76cfcb56bb065`.

The release is published, non-prerelease, immutable, and contains the complete
23-file distribution contract: five native binaries, one
checksum/provenance/license bundle per binary, and the project MIT, Apache-2.0,
and third-party notice files. At the time of this recovery qualification, the
extension runtime pin targeted `0.1.1`; PR #21 later advanced it to `0.1.2`.

### Hosted delivery qualification — macOS ARM64

`cargo xtask test-zed-hosted --timeout-seconds 180` passed against `v0.1.1`
using Zed 1.22.0 on macOS ARM64. The isolated profile resolved the protected
release tag to
`4019fcc2b63c26702ad7d77839f76cfcb56bb065`, downloaded and launched
`wit-language-server-aarch64-apple-darwin`, and verified SHA-256
`0bde485ebad8e71f1af071ed2a18adc8e67af4e91118f750b6483d8b1b2f7cb3`.
That digest matches the immutable GitHub Release asset.

The same qualification run passed all four hosted-delivery scenarios:

- first hosted install downloaded, verified, and launched the published server;
- cached install reused the same verified server and checksum without rewriting
  either cache file;
- deliberate executable corruption was detected and repaired with bytes matching
  the original published digest; and
- deletion of the checksum sidecar caused clean recovery, restoring the checksum
  and launching the server with the published release build identity.

This qualifies hosted delivery for `v0.1.1` on macOS ARM64. It does not
substitute for platform-specific hosted-delivery execution on macOS x86_64,
Linux GNU ARM64, Linux GNU x86_64, or Windows x86_64 MSVC.

## v0.1.0 LSP-only publication — 2026-10-07

> Historical evidence: the immutable `v0.1.0` GitHub Release was later deleted.
> Its protected tag remains, but GitHub permanently reserves tag names previously
> used by immutable releases, so the release cannot be recreated under
> `v0.1.0`. Distribution recovery proceeds with `v0.1.1`.

The first immutable GitHub release, `v0.1.0`, published only the native WIT
language server and its release metadata. It did not publish the Zed extension
or modify the Zed extension registry. The release was published from
`cb8dd60c7a0b74c93f1c5d62934ec8fb6d4d9d78` by CD
[run 37559191579](https://github.com/chiploom/zed-wit/actions/runs/37559191579).

The release tag resolves directly to that commit. The publication run passed
release-candidate validation, all five native build/test jobs, combined artifact
verification, artifact attestation, tag creation, draft upload, and final
publication verification. The immutable release contains the exact 23-file
distribution contract: five binaries, one checksum/provenance/license bundle per
binary, and the project MIT, Apache-2.0, and third-party notice files.

The publish job re-verified all 20 target-specific artifacts immediately before
publication and produced GitHub build-provenance attestation
`53410060` for those 20 subjects through Sigstore/Rekor. Hosted delivery must be
qualified separately with `cargo xtask test-zed-hosted`, which uses a fresh
isolated Zed profile and the published LSP release matching
`crates/wit-language-server/Cargo.toml`.
Record that run below before claiming hosted-delivery qualification. Registry
publication remains separate and is not claimed by this evidence.

### Hosted delivery qualification — macOS ARM64

The dedicated hosted-release qualification passed on macOS ARM64 against
`v0.1.0` using Zed 1.22.0. The test ran from post-release hardening head
`950d4a29fb460f3b6fc70ffc09678e18980f02b5` while resolving the published
release tag to
`cb8dd60c7a0b74c93f1c5d62934ec8fb6d4d9d78`.

`cargo xtask test-zed-hosted --timeout-seconds 180` used a fresh isolated Zed
profile with no project-local LSP binary override and no WIT language server on
`PATH`. Zed downloaded and launched
`wit-language-server-aarch64-apple-darwin` from the published release. The
downloaded executable's SHA-256 was
`362aa98b73f082bb51479afbb100556486c8de259719d30d5683e45ef478cd5b`,
which matches the immutable GitHub release asset digest.

The same run passed all four hosted-delivery scenarios:

- first hosted install downloaded, verified, and launched the published server;
- cached install reused the verified binary and checksum without rewriting them;
- deliberate executable corruption was detected and repaired with bytes matching
  the original published digest; and
- deletion of the checksum sidecar caused clean recovery, restoring the checksum
  and launching the server with the published release build identity.

This qualifies hosted delivery for macOS ARM64. It does not substitute for
platform-specific hosted-delivery execution on macOS x86_64, Linux GNU ARM64,
Linux GNU x86_64, or Windows x86_64 MSVC.

## Public repository transition — 2026-10-07

The repository is now public. The default branch is protected by an active
ruleset that requires pull requests, conversation resolution, an up-to-date
branch, the six PR CI checks, squash-only merging, and linear history while
blocking deletion and non-fast-forward updates. A separate active `v*` tag
ruleset prevents release-tag updates and deletion while allowing new release
tags to be created.

The GitHub `release` environment has been created for the publishing workflow.
Its deployment/reviewer protections remain release-gate settings that must be
verified before publication.

Hosted CI on public `main` completed successfully for
`35daeb10e11369af4cdc1132728567be8d0f2410` in
[run 37554729191](https://github.com/chiploom/zed-wit/actions/runs/37554729191).
At the time of the public transition this established hosted-CI evidence before
the first GitHub release. The later `v0.1.0` publication evidence above supersedes
the release-status portion of that historical record; Zed registry publication is
still not claimed.

## Restricted Mode qualification correction — 2026-10-06

A final macOS qualification run exposed Zed's new-worktree trust modal even
though the previous smoke/GUI harness could still reach its file/process
assertions. That made the pass criteria incomplete: Restricted Mode suppresses
project `.zed/settings.json` and can block language-server startup.

The harness now writes `session.trust_all_worktrees = true` to the isolated
profile's global `config/settings.json` before launching Zed. This follows
Zed's documented auto-trust mechanism and is scoped only to the disposable
`--user-data-dir`; it does not persist a manual trust grant in the user's real
profile. Both smoke and GUI evidence report the auto-trusted profile state, and
an xtask regression test verifies that the setting is present while preserving
GUI-specific settings.

A Restricted Mode / "Unrecognized Project" prompt during either automated Zed
qualification is therefore a failed or invalid run, not something the operator
should click through.

## PR #6 final merge re-audit — 2026-10-06

The final merge audit separates previously validated product/runtime code from
later qualification-tooling changes:

- Core extension, distribution, WIT analysis and language-server source are
  unchanged from `51f13af44fbf3906a5c4162edb8faccc381a16ef`, where the
  complete local gate, native Apple Silicon nextest run, packaging/license
  checks and automated real-Zed smoke all passed.
- The real GUI snippet/outline qualification subsequently passed on macOS using
  the cross-platform `test-zed-gui` harness. Later changes are confined to CI,
  GUI qualification tooling/dependencies, dependency-policy coverage and
  qualification documentation.
- The GUI input helper pins published `enigo` 0.6.1 and uses separate native,
  X11, Wayland and libei features. CI clippy-checks the applicable backend on
  every supported runner/architecture and both additional Linux Wayland
  backends.
- `check-dependencies` resolves `--all-features`, so optional GUI backend
  dependencies are included in repository source/license policy instead of
  escaping the audit through disabled default features.
- Synthetic GUI input remains explicit opt-in. Failed shortcut injection
  attempts release every modifier that was successfully pressed and report any
  cleanup failure.

The PR #6 merge gate required an exact-final-HEAD run of repository policy,
formatting, workspace clippy/tests, native target tests/build identity,
packaging/licenses, `cargo xtask test-zed`, and the macOS
`cargo xtask test-zed-gui --allow-input-injection true` qualification. Hosted
GitHub Actions were also expected on the exact candidate when account billing
allowed it; when operationally unavailable, the exception had to be recorded
instead of presenting historical hosted runs as final-candidate evidence.
Published release install/cache/corruption scenarios remained release-gated and
were not a PR merge prerequisite.

## PR #6 semantic editor candidate

- Validation date: 2026-10-06
- Validated implementation HEAD: `31d540950b1265dbe7ed4a8f9a694776b89f9bf6`
  (alias-aware signature rendering and parameter-span source spelling).
- Host: macOS ARM64 (`aarch64-apple-darwin`)
- Rust: `rustc 1.99.0 (b940084d7 2026-09-28)`; Cargo 1.99.0
- Exact local gate: passed on this implementation HEAD. It includes
  `cargo xtask check-no-python`, `cargo xtask check-dependencies`, formatting,
  workspace clippy, workspace tests, workspace check, Wasm build, native
  release build, and `git diff --check origin/main...HEAD`.
- Validation-evidence HEAD: `b7c070b682fafadbb0c5855b5277af9c2eeb61c7`;
  the same complete gate was rerun after the evidence update.
- A temporary self-hosted CI runner-routing experiment was added after this
  evidence and then fully reverted. At
  `5fb68459e0e83247f9fd02c598d10371a9c76693`, the net tree relative to
  validated implementation `31d540950b1265dbe7ed4a8f9a694776b89f9bf6`
  differed only in this validation document; no executable source or effective
  workflow changes remained.
- Subsequent maintenance advances `taiki-e/install-action` from v2.87.25 to
  v2.87.26 in CI/release workflows, corrects this provenance record, and adds
  reusable manual qualification fixtures under `tests/manual-zed/`. Those
  changes do not modify extension or native-server executable source; the WIT
  additions are test inputs only. At this point in the historical candidate
  record, exact-candidate local validation and hosted CI were still pending.
- Workspace tests: 67 passed (22 `wit-analysis`, 4 language-server unit, 17
  stdio protocol, 11 syntax/editing, 7 `xtask`, 6 adapter/distribution); no
  doctests were present.
- Build identity: passed; server reported
  `wit-language-server 0.1.0+git.31d540950b1265dbe7ed4a8f9a694776b89f9bf6`.
- Host packaging: not rerun for this candidate. Earlier packaging for
  `f4d87ec72467283c846d76e3b5cf5625915a4128` produced a 116-package license
  report, but is not treated as packaging evidence for this candidate.
- Zed: `zed --version` reports `Zed 1.22.0` on `/Applications/Zed.app`. A
  user-provided RPC trace shows fixture diagnostics behavior; it does not include
  server build identity or a retained artifact path, so it is partial smoke
  evidence rather than exact-candidate GUI signoff. Semantic/editor checks remain
  pending; see [manual testing](manual-testing.md).
- Hosted CI: no successful check run is recorded here for the exact final
  candidate. Historical runs on other SHAs are not candidate evidence. At this
  point, exact-candidate hosted CI was still pending and remained subject to
  GitHub Actions availability.

The complete gate passed on implementation HEAD
`31d540950b1265dbe7ed4a8f9a694776b89f9bf6` and was rerun on
validation-evidence HEAD `b7c070b682fafadbb0c5855b5277af9c2eeb61c7`.
The self-hosted runner experiment was fully reverted. At
`5fb68459e0e83247f9fd02c598d10371a9c76693`, the effective executable/workflow
tree remained unchanged from the pre-experiment candidate. The later v2.87.26
Action-pin maintenance is workflow-only, while `tests/manual-zed/` and its
documentation are qualification-only additions; none alter extension/server
executable source. The remaining exact-candidate gate at that point used the
following commands, with hosted CI tracked separately when available:

```sh
cargo xtask check-no-python
cargo xtask check-dependencies
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo check --workspace --locked
cargo build --target wasm32-wasip2 --locked
cargo build -p wit-language-server --release --locked
git diff --check origin/main...HEAD
cargo build -p wit-language-server --release --locked --target aarch64-apple-darwin
cargo xtask package-release --target aarch64-apple-darwin --output dist-validation
cargo xtask collect-licenses --target aarch64-apple-darwin --output dist-validation
```

The package was written under `dist-validation/` to avoid overwriting `dist/`.
Temporary packaging output was removed after recording its results.

## First automated Zed smoke trial at `d28af4836a25bc9a80f8cafaa0009e4621cebe0d`

The user's first run of `cargo xtask test-zed` on this SHA confirmed the
11 syntax/editing tests, 17 stdio protocol tests and 9 `xtask` tests passed.
The real-Zed phase timed out after 30 seconds before the harness observed the
native server process. The same run also showed `cargo fmt --check` differences
in the newly added smoke source. The subsequent harness fix applies rustfmt's
reported layout, forces `zed --new` for deterministic fresh-profile opening,
stages the extension as a dev-extension symlink, increases the default timeout
to 60 seconds and includes captured Zed logs in smoke failures.

This trial is failure evidence for `d28af483...`, not qualification evidence
for the subsequent fix.

A second smoke run on `5a8acf664535d12122808b85c17607cd73b0cbec`
passed the formatting, `xtask` unit, clippy, syntax/editing and stdio protocol
stages, but the real-Zed phase again timed out. The captured foreground log
contained only `zed is already running`, proving the remaining blocker was
Zed's stable-channel global single-instance guard rather than extension/server
startup.

A third smoke run on `29ee20e4efbc2ef05af87362fa878f790d3e214d`
successfully launched the isolated stateless Zed instance and loaded the WIT
extension, but Zed reported the expected unreleased-server 404. That proves the
fresh stateless worktree did not apply the project-local LSP binary override
before the adapter selected a server.

The next run on `fa91c1da69e60c586646c39d06eb21fa6675e5a3`
passed the real-Zed smoke end to end. The generated report recorded
`result: passed`, Zed 1.22.0, server PID 91133, and exact server identity
`wit-language-server 0.1.0+git.fa91c1da69e60c586646c39d06eb21fa6675e5a3`.
This demonstrates that isolated stateless Zed loaded the staged WIT extension,
resolved the exact release-built native server through the worktree PATH, and
started that server successfully. The same shell invocation still exposed one
rustfmt-only difference in `zed_smoke.rs`; the following commit applies that
formatting change, so a final complete gate is still required.

## Exact local validation run at `1ab100abe7aa5a278828345bdc2283295b201fa4`

On 2026-10-06, the user reran the full local gate on
`1ab100abe7aa5a278828345bdc2283295b201fa4` after the reusable manual fixtures
and Action-pin maintenance were present.

Recorded results:

- repository policy checks passed, including no-Python and dependency/license
  validation;
- both workflows referenced `taiki-e/install-action` v2.87.26 at
  `f7e5d7c961414b23f5b25b2da9294395d08513ad`;
- 67 workspace tests passed: 22 `wit-analysis`, 4 language-server unit, 17
  stdio protocol, 11 syntax/editing, 7 `xtask`, and 6 adapter/distribution;
- all workspace doctest targets completed with zero doctests and no failures;
- workspace check, Wasm extension build and native release build passed;
- the server reported
  `wit-language-server 0.1.0+git.1ab100abe7aa5a278828345bdc2283295b201fa4`;
- `git diff --check origin/main...HEAD` produced no error; and
- Apple Silicon packaging and license collection completed with a 116-package
  license report.

This is exact evidence for that SHA. At that point, the later Zed-automation
tooling still required its own final gate; subsequent sections preserve the
later qualification evidence separately.

## User-provided Zed fixture diagnostics trace

On 2026-10-06, the user supplied RPC logs from opening fixture files in Zed.
The trace shows empty diagnostics for the current annotation, async and core
fixtures, gated feature and nested-package fixtures, and the getter/setter
grammar-gap fixture. The legacy named-results fixture reports the expected
`wit-parser` diagnostic, `expected a type, found '('`, at line 2, characters
21–22. Code-action requests at empty ranges with no supplied diagnostics return
empty arrays.

The trace does not include the running server's build identity or a saved log
artifact path. Do not use it as proof of semantic hover, completion, definition,
references, formatting, or exact-final-SHA GUI qualification. Those manual
scenarios remain pending in [manual testing](manual-testing.md).

## Snapshot

- Date: 2026-10-01
- Hosted CI baseline: `9875ce49de42849f97fcb3ab2b1fed7a162fe1f9`
- Locally validated implementation: `4ded40a17f2deb893af38a0c63c6ae3b5c2fef24`
- Rust: 1.99.0
- GitHub Actions CI run: [#28](https://github.com/chiploom/zed-wit/actions/runs/36940382890)
- CI conclusion: success

The hosted matrix below applies to the `main` baseline. The local Apple Silicon
validation applies through `4ded40a17f2deb893af38a0c63c6ae3b5c2fef24`, which includes
the closed-sibling diagnostics fix, corrupt-cache recovery, package-isolated
fixtures, formatter regression fixes, project-local Zed settings, and native
server build-commit reporting. An earlier LSP lifecycle experiment was fully
reverted and has no effective source diff against `main`.

## Hosted CI evidence

Run #28 completed successfully on the baseline commit with the following jobs:

| Scope                          | Platform / target                            | Result |
| ------------------------------ | -------------------------------------------- | ------ |
| Quality                        | Ubuntu 24.04                                 | Passed |
| Native tests                   | macOS 15 / aarch64-apple-darwin              | Passed |
| Native tests                   | Ubuntu 24.04 / x86_64-unknown-linux-gnu      | Passed |
| Native tests                   | Windows 2025 / x86_64-pc-windows-msvc        | Passed |
| Portability check              | macOS 15 Intel / x86_64-apple-darwin         | Passed |
| Portability check              | Ubuntu 24.04 ARM / aarch64-unknown-linux-gnu | Passed |
| Release artifact build         | aarch64-apple-darwin                         | Passed |
| Release artifact build         | x86_64-apple-darwin                          | Passed |
| Release artifact build         | aarch64-unknown-linux-gnu                    | Passed |
| Release artifact build         | x86_64-unknown-linux-gnu                     | Passed |
| Release artifact build         | x86_64-pc-windows-msvc                       | Passed |
| Combined artifact verification | Ubuntu 24.04                                 | Passed |

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

On an `aarch64-apple-darwin` development host using Rust 1.99.0, the full
repository gate passed on `4ad1e9ba198c64b6f69ae1aa26a1c4ce8e86bd73`. The only
failure was `git diff --check`, which found one trailing blank line in
`languages/wit/highlights.scm`. Commit
`4ded40a17f2deb893af38a0c63c6ae3b5c2fef24` removed only that blank line, after
which `cargo fmt --all -- --check`, `cargo test -p wit-syntax --locked`, and
`git diff --check origin/main...HEAD` all passed.

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
tests, 4 stdio integration tests, 10 syntax/editing tests, 7 `xtask` tests, and
6 adapter/distribution tests, all passing. The release language-server build
also completed successfully.

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
  clean;
- getter/setter grammar-gap highlighting recovering accessor names, `get`/`set`
  keywords, setter parameters, and builtin/user-defined accessor types.

The exact Zed editor version was not recorded in the supplied evidence, so this
is a development-extension smoke/behavior record rather than a complete signed-off
GUI qualification matrix.

## What remains unqualified

The following remain pending and must not be inferred from the green CI matrix:

- A complete manual GUI qualification record with the exact Zed editor version,
  screenshots/log paths, and every checklist row marked with evidence.
- End-to-end automatic native-server download from an actual GitHub release.
- Corrupt/missing hosted asset behavior against a published release.
- Execution of the protected CD workflow and artifact attestations.
- Registry succession from the existing `wit` extension.
- Registry publication and succession remain pending even though the repository
  is now public.

Use [manual testing](manual-testing.md) for the editor qualification procedure.
The repository is public; the remaining registry gate is the ownership/succession
process for the existing `wit` entry documented in [publishing](publishing.md).

## Release gate

Before each release candidate is considered ready:

1. Keep CI green on the exact candidate commit.
2. Complete and record the applicable manual Zed scenarios.
3. Verify public-repository security settings, immutable releases,
   release-environment protection, and the active default-branch and `v*` tag
   rulesets.
4. Resolve the existing `wit` registry ownership/succession requirement.
5. Dispatch the protected CD workflow from the default branch with the intended
   scope and tag format (`vX.Y.Z` for LSP or `v-extension-X.Y.Z` for the Zed
   extension), with `operation=validate`; require every scope-applicable gate to
   pass.
6. Dispatch CD again from the same commit with the same scope/tag and
   `operation=publish`; allow the protected publish job to create the immutable
   release tag only after all applicable build and verification jobs pass.
7. For LSP releases, verify the published five-target asset set, checksums,
   provenance, licenses, tag-to-commit binding and attestations. For extension
   releases, verify the tagged source and the pinned immutable LSP dependency.
8. Re-test the applicable Zed installation path against the published release.
