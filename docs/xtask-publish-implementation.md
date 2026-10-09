# Version-aware xtask publish: implementation contract

Status: **implementation in progress**. The Rust `cargo xtask publish` frontend
has been added on this draft branch. Local and cross-platform validation, threat
model review, and GitHub CD integration qualification are still required. This work belongs to [issue #24](https://github.com/chiploom/zed-wit/issues/24)
and is stacked on the xtask refactor in [PR #25](https://github.com/chiploom/zed-wit/pull/25).

The source of truth for existing release behavior remains [publishing.md](publishing.md).
The implementation must preserve the existing protected
`.github/workflows/release.yml` dispatch contract, protected-branch policies, five-target
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
The protected CD workflow receives an optional `expected_sha` from the frontend
and rejects any invocation whose resolved `GITHUB_SHA` differs. This closes the
time-of-check/time-of-dispatch race when `main` advances during remote checks;
manual dispatches that omit the optional input retain their existing contract.
The workflow run name includes its operation, scope and tag so the frontend can
distinguish completed validation dry runs from completed publication attempts
without guessing from the most recent run. Unlabeled historical runs remain
ambiguous and require manual review. Neither change relaxes the protected
`release` environment, release-gate tests, or native artifact checks.

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

A failed post-push PR submission can be retried only when the existing remote
preparation branch points to the same validated local commit. The tool must
not overwrite remote branch data or create duplicate review PRs. The human
operator inspects prior attempts when a PR already exists or a response is
ambiguous.

Remote read/write boundaries must be explicit so a failed preparation, timeout,
partial edit, stale default branch, retried dispatch, or duplicate PR cannot
silently advance to publication.

## Canonical GitHub CLI API host and authentication

The release CLI binds its GitHub operations to the same **GitHub.com** host as
its validated Git origin, including read-only checks and remote writes. All
`gh api` requests specify `--hostname github.com`; `gh repo view`,
`gh pr list` and `gh pr create` explicitly target
`github.com/chiploom/zed-wit`; `gh auth status` checks only `github.com`.
The wrapper rejects mismatching `GH_HOST` and `GH_REPO`, a noncanonical
per-host `api_host`, or a custom `http_unix_socket` before proceeding. It
retains the user's regular `GH_TOKEN`/`GITHUB_TOKEN` or stored GitHub.com
credentials and does not expose token data on authentication failures.

This is intentional fail-closed behavior: a user configured for GitHub
Enterprise must explicitly select their GitHub.com context rather than risk
sending a release request to an identically named Enterprise repository.
Disposable mock-GitHub-CLI tests exercise unpinned versus explicitly pinned
host routing and host-configuration conflicts without network or release writes.

## Race-resistant remote submission and dispatch

Release-preparation submission checks `remote.origin.pushurl` and expanded Git
`pushInsteadOf`/URL rewriting, requiring exactly one effective canonical
destination. It then pushes to that explicit verified URL with an empty
`--force-with-lease=<branch-ref>:` expected value: creation must be atomic
and fails if a concurrent actor creates the branch. The post-push and
post-PR checks revalidate exact remote and PR-head SHA. Recovery still accepts
only an already-existing remote branch pointing to the identical commit.

Before checking workflow history and dispatching CD, `--resume` creates an
exclusive lock file in the checkout's Git common directory. This prevents
simultaneous dispatch attempts from the same Git checkout or shared worktree.
It is a **local advisory lock**, not a distributed transaction: separate clones
or computers can still dispatch independently if GitHub run-list visibility
lags. When a stale lock remains after an interrupted process, inspect active
Actions runs before manually clearing it. The frontend never claims globally
at-most-once dispatch.

The existing serialized, environment-protected CD workflow is the final
publication authority. Its protected publish job rechecks the unpublished
draft, scope/title and exact release tag commit immediately before promoting
the release; an already-published or moved release fails closed. The existing
native build, five-target verification, attestations, optional reviewer gates and
immutable-release enforcement are unchanged. Because GitHub Actions allows
pending concurrency runs to be replaced, a dispatch acceptance is never
reported as successful publication.

On resume, same-scope version monotonicity is reevaluated against all known
remote tags/releases. Only the exact verified draft under safe recovery may
be excluded from the candidate collision check; any different same-scope
version at or above the candidate still blocks the dispatch.

## Mandatory protection qualification (fail closed)

**Temporary zero-reviewer policy:** release preparation still requires an
effective `main` pull-request rule, linear history and all six required CI
checks, but its approving-review count may be **zero**. The protected
`release` environment may likewise have **zero** required reviewers;
self-review prevention is not required when no approval is configured.
The environment must still exist, expose readable protection metadata, and
restrict deployment to **protected branches only**. Reviewer requirements
can be restored later without changing the publishing protocol.

`--submit --confirm` reads the effective main-branch rules using
`GET /repos/chiploom/zed-wit/rules/branches/main`; `--resume --confirm`
rechecks them and reads
`GET /repos/chiploom/zed-wit/environments/release`. GitHub applies
repository-level and organization-level active rules to the branch-rules
response. If the user's GitHub token cannot read either endpoint, the
response is malformed, or a required protection is absent, the command
refuses its remote write. Operator inspection is necessary to fix permissions
or protection settings. This does not create or modify a ruleset/environment.

**Verified repository state (2026-10-09):** the active repository ruleset
`Protect main` (ID `24619696`) requires a pull request, strict six-check
CI gating, and linear history, but `required_approving_review_count=0`.
An active `Protect release tags` ruleset (ID `24620024`) blocks
tag deletion/update and non-fast-forward modifications. No inherited
organization ruleset was returned by the current ruleset listing.
An authenticated environment inspection subsequently confirmed **zero**
required reviewers, no self-review prevention and custom deployment-branch
policies instead of protected branches. The zero-reviewer configuration is
now permitted; the **deployment-branch policy remains noncompliant** and
continues to block CD resume/publication. No repository settings were changed.

Read-only qualification commands using an appropriately authorized local
GitHub CLI login:

```sh
gh api --paginate 'repos/chiploom/zed-wit/rules/branches/main?per_page=100'
gh api 'repos/chiploom/zed-wit/environments/release'
gh api --paginate 'repos/chiploom/zed-wit/rulesets?includes_parents=true&per_page=100'
```

The frontend and **protected CD publish job** revalidate the required
pull-request, CI, linear-history and deployment-branch restrictions using
live GitHub API data. Zero required reviews are accepted by both. API denial,
incomplete protection data, or missing required branch/CI safeguards fails
closed, including on direct/manual workflow dispatch. GitHub-native reviewer
gates, if subsequently configured, remain honored.

## Reviewed PR provenance and guarded recovery

Before dispatching a publication, `--resume` fetches the merged preparation
PR's **complete paginated file list**, compares it with GitHub's reported file
count, and requires a reviewed scope-specific version change in each manifest's
patch. It checks the predecessor-to-candidate transition, the reviewed PR head,
the merged commit, and the checked-out protected `main` manifest. It supports
merge, squash, and rebase layouts but rejects missing patches, contradictory
history, unexplained changes to the version manifest, or an ambiguous
predecessor. These checks establish release preparation provenance; branch
names and PR titles are never sufficient evidence.

Draft recovery resolves a lightweight tag directly and an annotated tag via
its peeled `^{}` commit ref. The tag must target the exact validated source
commit. A completed, known failed, cancelled, or timed-out **publication** run
is required before dispatch can retry an unpublished draft. A previous success,
active run, missing history, ambiguous conclusion, malformed record, or
published release blocks an automatic retry. Other scopes and completed
validation-only runs are distinguished by explicit run names. Protected CD
remains responsible for idempotent publication and immutable-release gates.

`--wait` checks the exact dispatch run ID, source commit, protected branch,
workflow path (including GitHub's `@main` suffix), scope, operation, and tag.
It never infers success from a completed validation-only run. A successful
workflow must still be followed by verification of the non-draft immutable
GitHub Release.

All mutation tests must run in disposable Git repositories and Cargo fixtures,
with mocked GitHub responses. Never test these stages by dispatching the
production workflow or creating production release branches.

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
