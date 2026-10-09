# Publishing and maintenance

The repository is public and protected tag `v0.1.0` identifies the first
LSP-only release. Its immutable GitHub Release was deleted, and GitHub permanently
reserves tag names that were used by immutable releases, so `v0.1.0` cannot be
recreated. Recovery therefore proceeds with a new patch version. GitHub release
publication does not grant permission to replace the existing Zed registry
entry. Repository rulesets, immutable releases, and release-environment
protections remain part of the release contract for every subsequent version.

## Release scopes

CD supports two independent release streams:

| Scope | Tag format | Version source | GitHub release payload | Zed registry publication |
| --- | --- | --- | --- | --- |
| `lsp` | `vX.Y.Z` | `crates/wit-language-server/Cargo.toml` | Five native LSP binaries plus checksums, provenance, redistribution notices and project licenses | No |
| `extension` | `v-extension-X.Y.Z` | `extension.toml` + adapter crate | Tagged extension source; no native LSP binaries attached | No |

The adapter and `extension.toml` versions remain aligned with each other, but
the native language-server version is independent. The extension's runtime LSP is
an explicit pin at
`package.metadata.zed-wit.runtime-lsp-version` in the root `Cargo.toml`.
The adapter embeds that pin at build time and downloads the corresponding
`vX.Y.Z` LSP release.

Changing `crates/wit-language-server/Cargo.toml` therefore does **not** change
the extension's runtime dependency. Publish and qualify the new LSP release
first. Then update the runtime pin in a separate extension change, test the
hosted install path, and release the extension when appropriate. This ordering
prevents protected `main` from ever pointing the extension at an unpublished
LSP version.

An `extension` release does not duplicate LSP binaries. Before CD can publish
it, the explicitly pinned LSP release must already be published,
non-prerelease, immutable, and contain every supported server binary plus
checksum.

Both tag formats begin with `v`, so the existing protected `v*` tag ruleset
covers both streams. GitHub extension releases still do not publish or replace
the Zed extension registry entry; registry succession remains a separate process.

Protected tag `v0.1.0` represents the historical first **LSP-only** release,
but its deleted immutable GitHub Release cannot be recreated under that tag.
`v0.1.1` is the published immutable recovery release for restoring LSP
distribution, and the extension runtime pin now targets `0.1.2`. Neither the
LSP release nor this runtime pin publishes a Zed extension.

## Native assets

The adapter expects each server executable at
`https://github.com/chiploom/zed-wit/releases/download/v<RUNTIME_LSP_VERSION>/<asset>`
and its `<asset>.sha256` sidecar. The runtime version comes from the explicit
root-manifest pin, not from the native server crate's current package version.
Extension version bumps do not require a new LSP release when the pin is
unchanged. Archives are not interchangeable with these raw downloads.

| Platform | Target | Asset |
| --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | `wit-language-server-aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` | `wit-language-server-x86_64-apple-darwin` |
| Linux x86_64 GNU | `x86_64-unknown-linux-gnu` | `wit-language-server-x86_64-unknown-linux-gnu` |
| Linux ARM64 GNU | `aarch64-unknown-linux-gnu` | `wit-language-server-aarch64-unknown-linux-gnu` |
| Windows x86_64 MSVC | `x86_64-pc-windows-msvc` | `wit-language-server-x86_64-pc-windows-msvc.exe` |

Builds use native GitHub-hosted runners and the pinned Rust toolchain. The Linux
build image is Ubuntu 24.04; do not claim compatibility with older glibc versions
without testing them. Native Windows ARM64, Linux musl and other targets are not
in this release matrix. macOS signing/notarization and Windows signing are not
configured or claimed.

`cargo xtask package-release --target <target> --output dist` packages the
already-built `target/<target>/release/wit-language-server[.exe]`. Each asset gets
one checksum line, `<64 lowercase hex digits>  <exact asset filename>`, followed by
a newline, a `<asset>.provenance.json` build record, and an
`<asset>.licenses.txt` redistribution-notice bundle for the target's native
dependency closure and pinned Rust standard library. Build provenance JSON is not
itself a cryptographic attestation.

## Immutable-release enforcement (protected CD)

Before creating or resuming any GitHub Release, the protected `release`
job requires GitHub's Administration-read immutable-release status endpoint
to confirm `enabled=true`; it repeats this check immediately before
promoting the draft, and afterward requires `immutable=true` on the
published GitHub Release. A denial, 404, malformed or disabled response
blocks publication rather than assuming tag rules alone are sufficient.

Because `GITHUB_TOKEN` has no configurable Administration-read scope,
an authorized administrator must provision the environment-scoped
`RELEASE_POLICY_READ_TOKEN` secret with a narrowly scoped fine-grained token
or GitHub App token granting **repository Administration: read only**.
It is used solely for policy GETs and must never be logged. Missing or
expired credentials fail closed. The currently enabled status has **not**
been verified through the connected GitHub app.

The active `main` ruleset already uses strict CI checks bound to GitHub
Actions integration ID `15368`, and the frontend and CD now require
that exact trusted publisher for each of the six release CI contexts even
when checks appear across multiple applicable rulesets.

The version-aware submit/resume preflight and protected CD also require active
repository or inherited **tag rulesets** covering both release streams via
`refs/tags/v*` (or `~ALL`), with no excluded release tags or bypass actors.
Effective rules must prohibit updates, deletions and non-fast-forward changes.
A matching `creation` restriction is rejected because protected CD must be able
to create the next release tag. These requirements can be satisfied by layered
rulesets and are checked by effective policy, not a hardcoded ruleset ID.
Other patterns may be secure, but the automated check conservatively rejects
patterns whose complete coverage cannot be established.

## Release authorization requirements

**Temporary policy: zero required reviewers are permitted** for both
release-preparation PRs and protected CD publication. The effective `main`
rules must still require a pull request, linear history and all six required
CI checks. The `release` environment must exist and restrict deployment to
**protected branches only**. The xtask frontend and protected CD fail closed
on missing or unreadable rule/branch-policy data.

As inspected on 2026-10-09, `Protect main` requires zero approvals, which is
now acceptable. An authenticated read confirmed that `release` has no
required reviewers, but uses **custom branch policies** rather than
protected-branches-only deployment. The latter is still a blocker; its
configuration must be reviewed separately. No repository settings were
changed. GitHub's native reviewer gate still applies if reviewers are
configured in the future.

The reviewed release-preparation PR also defines the **exact release source
commit**: its GitHub-reported merged SHA (including normal squash merges)
must equal the current protected `main` SHA at resume. Even an otherwise
legitimate later change to code, Cargo.lock or release notes requires a
new reviewed preparation for that source. A future explicitly reviewed
requalification protocol may expand this rule; this implementation does
not silently requalify newer source revisions.

## Version-aware preparation frontend

The optional `cargo xtask publish --scope lsp|extension` command provides a
**read-only version plan** by default. See [xtask publish stages](xtask.md#version-aware-release-preparation-and-protected-cd)
and the [implementation contract](xtask-publish-implementation.md).
It does not create tags, GitHub Releases, registry submissions or attestations.

The only authorized path to a new publication is:

1. Run a read-only plan on clean, current `main`. Check exact next version,
   candidate tag, remote collision history and expected release note files.
2. Use `--prepare --confirm` to create a local branch, bump only the chosen
   release stream and `Cargo.lock`, and generate **unreviewed** notes/changelog
   placeholders. No PR or publication is created by this stage.
3. Have a human replace the placeholders, review and commit the scoped changes,
   then explicitly invoke `--submit --confirm` to run checks, push the release
   branch, and open a PR. Review and merge it through normal `main` protections.
4. On the new protected `main`, use `--resume --pr N --confirm` to revalidate
   the exact merged preparation PR and request **only** the existing
   `release.yml` workflow with `operation=publish`. `--wait` watches the
   exact returned run ID, never an inferred most-recent run.
5. CD remains responsible for protected tags, environment approval,
   attestation, immutable GitHub Releases and the five-target native artifact
   contract; no local xtask action may bypass it.

For PR submission, effective Git push URLs (including `pushurl` and rewrite
rules) must identify only the canonical repository. A new branch is pushed
with an explicit expected-absent-ref lease, not a plain fast-forward push.
The remote ref and GitHub PR head are verified against the validated commit.

For publication requests, the checkout's exclusive Git lock prevents
simultaneous local resume calls, while CD's protected serialization and
last-moment draft/tag check prevent duplicate release promotion. No
distributed exactly-once dispatch claim is made for separate hosts or
temporary API inconsistency. Inspect existing Actions runs before retrying
ambiguous dispatch outcomes.

A successful dispatch means **requested**, not published. Missing/ambiguous
GitHub history, an existing non-draft tag, duplicate active CD runs, denied
permissions, or a reserved immutable tag requires human inspection rather than
blind retry. Because deleted immutable-release tags may be invisible to listing
APIs, the existing protected workflow remains the authoritative final gate.
LSP upgrades do not change the extension runtime LSP pin automatically.

## Release gate

The CD workflow is dispatched from the protected default branch with either
`lsp` or `extension` scope. LSP releases use `vX.Y.Z`; extension releases use
`v-extension-X.Y.Z`. The selected tag must match the version source for that
scope and must not already identify a published release.

The workflow's `operation` input controls lifecycle behavior:

- `validate` performs a dry run for a **new** tag and creates nothing.
- `publish` qualifies and publishes a new tag/release. It may also resume an
  existing unpublished draft when the tag still resolves to the validated
  release commit. An existing tag with no draft is rejected because GitHub does
  not expose a safe distinction between an interrupted tag-only publication and
  a tag name permanently reserved by a deleted immutable release.

Deleted immutable releases are not recoverable under the same tag name. GitHub
reserves that tag name permanently after deletion, so publish a new corrected
version instead.

All operations execute repository policy, formatting, clippy/check/doctests, and
release identity validation. The `lsp` scope runs native tests on all five
release targets, packages the server assets/notices, and verifies the complete
LSP artifact set. The `extension` scope verifies the referenced immutable LSP
dependency and builds the Zed extension for `wasm32-wasip2`.

For publication:

1. Verify the active default-branch and `v*` tag rulesets, the protected GitHub
   `release` environment, private vulnerability reporting, and repository
   **immutable releases**. Existing `v*` tags must be non-updatable and
   non-deletable; creation must remain available to the CD job. Immutable releases
   lock the published release assets and associated tag after publication.
2. Complete normal CI and the applicable [manual matrix](manual-testing.md) on the
   intended release commit. Audit locked dependency licenses and retain
   `Cargo.lock`.
3. Ensure the selected scope's version source, changelog, compatibility notes,
   and nonempty `docs/releases/<scope>/vX.Y.Z.md` user-facing release notes
   agree. For extension releases, also verify the adapter's pinned LSP version is
   intentional; CD independently verifies that the referenced LSP release exists
   and is immutable.
4. Optionally dispatch **CD** from the default branch with the intended scope,
   its matching tag format, and `operation=validate` as a release-candidate dry
   run. Require every job applicable to that scope to pass.
5. Dispatch **CD** from the intended release commit with the same scope/tag and
   `operation=publish`. The protected publish job reruns the applicable gate,
   explicitly creates the protected lightweight tag at the validated SHA,
   verifies that binding, creates a scoped draft release, then publishes it.
   New LSP releases additionally generate standard SLSA build-provenance
   attestations and upload the verified native asset set; extension releases
   publish source only. If `main` advanced after an earlier dry run, this
   publication run is the authoritative qualification.
6. If publication is interrupted after the draft exists, rerun CD from the same
   validated commit with `operation=publish`. CD resumes only an unpublished
   matching draft whose tag still resolves to the validated SHA. An existing tag
   with no draft is rejected; investigate it manually rather than risking reuse
   of a tag name reserved by a deleted immutable release.
7. If a published immutable release is deleted, do **not** attempt to recreate it
   under the same tag. GitHub permanently reserves tag names previously used by
   immutable releases. Publish a corrected new version, then update any runtime
   pins after that new release is qualified.
8. For LSP releases, download each published asset and sidecar, verify checksum,
   provenance and target-specific redistribution notices, and run
   `cargo xtask test-zed-hosted` on each available supported host. Also run
   `gh attestation verify <asset> --repo chiploom/zed-wit` against downloaded
   bytes. For extension releases, verify the tagged source installs as a
   development extension and resolves the already-published pinned LSP. The
   adapter itself verifies SHA-256, not attestations.

Checksums and executable bytes come from the same GitHub origin. They detect
corruption and asset mismatch, not a compromised publishing account. Immutable
action pins, restricted publishing permission, protected tags, environment
approval, and provenance reduce separate supply-chain risks.

A failed dry run is not a release. Fix the cause and rerun the complete workflow.
A published release is immutable by policy: do not replace its tag or assets.
For a bad published release, document the problem and publish a corrected version;
update the adapter pin and perform fresh-download verification again.

## Zed registry succession

The registry already has `wit` 0.4.0 pointing to `valentinegb/zed-wit`. Zed's
[publishing FAQ](https://zed.dev/docs/extensions/publishing/faq) requires either
written owner permission or documented contact attempts unanswered for at least
six weeks before replacing an unresponsive owner's extension. Repository age,
an independent rewrite, and a local dev installation do not satisfy this gate.

Discuss improvements with the existing owner first, then agree a migration or
successor proposal with Zed maintainers under the current policy. Keep written
evidence, attribution and the existing registry ID. Do not submit a duplicate
entry. No contact, permission, six-week wait, registry PR or transfer is claimed
by this repository's initial implementation. Those actions require a separately
authorized maintainer publication task.
