# Version-aware xtask publish: implementation contract

Status: **implementation in progress**. The Rust `cargo xtask publish` frontend
has been added on this draft branch. Local and cross-platform validation, threat
model review, and GitHub CD integration qualification are still required. This work belongs to [issue #24](https://github.com/chiploom/zed-wit/issues/24)
and is stacked on the xtask refactor in [PR #25](https://github.com/chiploom/zed-wit/pull/25).

The source of truth for existing release behavior remains [publishing.md](publishing.md).
The implementation must preserve the existing protected
`.github/workflows/release.yml` dispatch contract, required approvals, five-target
LSP distribution, release attestations, immutable releases, and the separate Zed
registry succession process.

## Public command and safety contract

Proposed interface, subject to implementation tests:

```text
cargo xtask publish --scope <lsp|extension> [--bump <patch|minor|major>] [--dry-run]
cargo xtask publish --scope <lsp|extension> [--bump <patch|minor|major>] --prepare --confirm
cargo xtask publish --scope <lsp|extension> --submit --confirm
cargo xtask publish --scope <lsp|extension> --resume --pr <number> --confirm [--wait]
```

- **Default means planning**, not remote writes or publication. A patch bump is
  the default. Display current and candidate SemVer, candidate tag, affected
  manifests, required release notes, and the exact proposed next operation.
- `--dry-run` is read-only. It cannot create files, branches, commits, PRs, tags,
  releases, or workflow runs, even if a later stage would do so.
- The implementation must define an explicit, independently authorized
  preparation operation before writing/pushing a branch and opening a PR. No
  implicit remote writes, even when the plan is valid.
- `--resume` is only valid after the specific release-preparation PR has merged
  onto the protected default branch. It must re-fetch state rather than trust a
  saved plan. `--confirm` is required before requesting protected publication.
  `--wait` must apply only to an explicitly dispatched workflow and must report
  that workflow's actual outcome.
- Reject incompatible options before any side effects. Return actionable errors
  for missing authentication/permissions, dirty or stale branches, unsupported
  versions, cancelled approvals, rate limits, and unavailable GitHub Actions.
- Do not print credentials, `GH_TOKEN`, or sensitive GitHub responses to logs.

## Release scope and version invariants

| Scope | Authoritative version sources | Candidate tag |
| --- | --- | --- |
| `lsp` | `crates/wit-language-server/Cargo.toml` | `vX.Y.Z` |
| `extension` | Root `Cargo.toml` and `extension.toml`, kept equal | `v-extension-X.Y.Z` |

Validate strict stable SemVer and checked arithmetic. Reject unsupported
prerelease/build metadata and numeric overflow; document the pre-1.0 bump policy.
Check *all* candidate-tag conflicts with protected refs, published and draft
releases, and the permanently reserved names of deleted immutable releases.
Where GitHub cannot conclusively establish availability, **fail closed** and
require a fresh candidate version. Never force-update tags or reuse ambiguous
historical names.

The LSP and extension are independent streams. Bumping the server version must
**never** automatically modify the extension's
`package.metadata.zed-wit.runtime-lsp-version` pin. Updating that pin requires
a separately qualified, published LSP and its own review.

## State transitions and ownership

1. **Plan:** Read-only discovery of Git identity, repository/default branch,
   current manifest versions, Cargo toolchain/lockfile, release notes, changelog,
   protected refs, release history, and existing pending preparation work.
   Produce a deterministic plan and list any uncertainty.
2. **Prepare (explicitly authorized):** Start a dedicated branch from the latest
   protected `main`. Update the scope's manifest version(s) and `Cargo.lock`
   through Cargo; create the correctly scoped versioned notes and changelog
   entries for *human review*. Do not fabricate release-note claims.
3. **Review:** Run the relevant existing policy, formatting, compilation, tests
   and release-check commands. With explicit authorization, push the preparation
   branch and open a **reviewable PR**, then report its URL/commit. Do not merge
   automatically and do not write to protected `main` directly.
4. **Resume:** Resolve the specific preparation PR, verify it merged and its
   expected release content/commit is on current `main`; compare again against
   remote tags, releases, protection settings, and configured release workflow.
   Refuse an outdated or ambiguous candidate.
5. **Dispatch on explicit confirmation:** Invoke **only**
   `.github/workflows/release.yml` with
   `operation=publish`, `scope=<lsp|extension>` and the exact candidate
   `tag`, dispatched from protected `main`. Never run `git tag`,
   `git push --tags`, `gh release create`, asset uploads, or registry PR
   submission locally. Leave CD's environment review, immutable release and
   attestation gates authoritative.
6. **Observe:** Without `--wait`, report the returned workflow run as
   *requested/pending*, never published. With `--wait`, identify the **exact**
   run associated with the dispatch (not merely the latest workflow), poll to
   a terminal conclusion, and differentiate validation, waiting for approval,
   failure, cancellation, and published success.

Remote read/write boundaries must be explicit so a failed preparation, timeout,
partial edit, stale default branch, retried dispatch, or duplicate PR cannot
silently advance to publication.

## Implementation sequence and regression evidence

- [ ] Add the `publish` command to the **existing**
  `crates/xtask/src/cli.rs` registry and dispatch, with a grouped domain module
  under `crates/xtask/src/tasks/`. Do not add another dispatcher.
- [ ] Implement a pure strict SemVer planner with overflow checks, independent
  scope handling, and deterministic tag formatting. Test 0.x minor/major
  policy, malformed/prerelease versions, and collisions.
- [ ] Model side effects behind narrow interfaces so read-only planning and
  invalid arguments can be tested without touching network or filesystem.
- [ ] Implement robust GitHub `gh`/Git state discovery with explicit
  repository, branch, protection, permissions, tag and release-history checks.
  Test missing auth, reserved names, stale branches, ambiguity and retries.
- [ ] Implement authorized preparation that uses Cargo for lockfile updates,
  stages only intended files, and requires human-authored release notes.
  Test error recovery and safe reruns without duplicate PRs.
- [ ] Implement authorized CD dispatch/resume with matching-run correlation
  and optional wait. Test unsuccessful dispatch, duplicate requests, cancelled
  approvals, and terminal workflow conclusions.
- [ ] Prove that no code path bypasses CD, environment approval, attestations,
  native asset checks, or Zed registry succession.
- [ ] Update `docs/xtask.md`, `docs/publishing.md`, `CONTRIBUTING.md`,
  `AGENTS.md` and the changelog **when executable functionality lands**.
- [ ] Validate on the pinned Rust toolchain with formatting, strict Clippy,
  workspace tests, release checks, and targeted isolated-destination tests.
  Record absent optional/hosted/cross-platform qualification honestly.

This document establishes security boundaries and the current CLI contract.
The issue remains open and PR #26 remains a draft. No release has been
requested or published; do not mark it ready to merge until implementation,
independent audits and full local/platform qualification are complete.
