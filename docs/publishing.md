# Publishing and maintenance

The repository is public and `v0.1.0` is published as an immutable GitHub
release. GitHub release publication does not grant permission to replace the
existing Zed registry entry. Repository rulesets, immutable releases, and
release-environment protections remain part of the release contract for every
subsequent version.

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

The existing `v0.1.0` release is an **LSP-only** release. Its attached assets
are the native language server and its metadata; it is not a published Zed
extension.

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

## Release gate

The CD workflow is dispatched from the protected default branch with either
`lsp` or `extension` scope. LSP releases use `vX.Y.Z`; extension releases use
`v-extension-X.Y.Z`. The selected tag must match the version source for that
scope and must not already identify a published release.

The workflow's `operation` input controls lifecycle behavior:

- `validate` performs a dry run for a **new** tag and creates nothing.
- `publish` qualifies and publishes a new tag/release. It may also resume an
  interrupted draft when the existing tag still resolves to the validated
  release commit.
- `regenerate` requires an **existing protected tag** and recreates a missing
  GitHub Release from that tag's original source commit. It never creates,
  updates, or deletes the tag.

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
6. If publication is interrupted after tag creation, rerun CD from the same
   validated commit with `operation=publish`. CD accepts an exact tag with no
   release or an unpublished matching draft only when the tag still resolves to
   the validated SHA, then resumes without moving or deleting the tag.
7. If a published GitHub Release is later deleted while its protected tag
   remains, dispatch CD from current protected `main` with the original scope
   and tag plus `operation=regenerate`. CD resolves and validates the existing
   tag commit, rebuilds the release from that source, uses the current scoped
   release notes, recreates the draft, verifies its scope-specific asset set, and
   republishes it without changing the tag. Because the workflow itself runs from
   current protected `main`, regenerated LSP artifacts use a signed custom
   regeneration attestation that explicitly records the protected source tag and
   source revision instead of claiming normal SLSA provenance from the control
   commit.
8. For LSP releases, download each published asset and sidecar, verify checksum,
   provenance and target-specific redistribution notices, and run
   `cargo xtask test-zed-hosted` on each available supported host. Also run
   `gh attestation verify <asset> --repo chiploom/zed-wit` against downloaded
   bytes. For regenerated LSP releases, also require predicate type
   `https://github.com/chiploom/zed-wit/attestations/release-regeneration/v1`
   and verify that its `artifact_source.tag` and `artifact_source.revision`
   match the protected release tag. For extension releases, verify the tagged
   source installs as a development extension and resolves the already-published
   pinned LSP. The adapter itself verifies SHA-256, not attestations.

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
