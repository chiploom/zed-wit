# Publishing and maintenance

The repository is public and `v0.1.0` is published as an immutable GitHub
release. GitHub release publication does not grant permission to replace the
existing Zed registry entry. Repository rulesets, immutable releases, and
release-environment protections remain part of the release contract for every
subsequent version.

## Native assets

The adapter expects the raw executable at
`https://github.com/chiploom/zed-wit/releases/download/v0.1.0/<asset>` and its
`<asset>.sha256` sidecar. Update adapter and manifest versions together for future
releases. Archives are not interchangeable with these raw downloads.

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

The CD workflow is dispatched from the protected default branch with a new stable
`vX.Y.Z` tag name. The tag must match the adapter, extension, and native-server
version; it does not exist before the workflow begins.

Set the workflow's `publish` input to `false` for a release-candidate dry run.
A dry run executes repository policy, formatting, clippy/check/doctests, the Wasm
adapter build, native tests on all five release targets, packaging, redistribution
notice generation, and complete artifact-set verification without creating a tag
or GitHub Release.

For publication:

1. Verify the active default-branch and `v*` tag rulesets, the protected GitHub
   `release` environment, private vulnerability reporting, and repository
   **immutable releases**. Existing `v*` tags must be non-updatable and
   non-deletable; creation must remain available to the CD job. Immutable releases
   lock the published release assets and associated tag after publication.
2. Complete normal CI and the applicable [manual matrix](manual-testing.md) on the
   intended release commit. Audit locked dependency licenses and retain
   `Cargo.lock`.
3. Ensure `Cargo.toml`, the native server manifest, `extension.toml`, the
   adapter release version, changelog, and compatibility notes agree.
4. Optionally dispatch **CD** from the default branch with the new `vX.Y.Z`
   tag and `publish=false` as a release-candidate dry run. Require the
   validation, five native builds, and combined artifact verification to pass.
5. Dispatch **CD** from the intended release commit with the same tag and
   `publish=true`. The protected publish job runs the full gate again, generates
   artifact attestations, explicitly creates the protected lightweight tag at the
   validated SHA, verifies that binding, creates a draft release from the existing
   tag, uploads the complete asset set, then publishes the draft. If `main`
   advanced after an earlier dry run, this publication run is the authoritative
   qualification.
6. If publication is interrupted after tag creation, rerun CD from the same
   validated commit with `publish=true`. CD accepts an exact tag with no release
   or an unpublished draft release only when the tag still resolves to the
   validated SHA, then resumes publication without moving or deleting the tag.
7. Download each published asset and sidecar, verify checksum, provenance and the
   target-specific redistribution notice, and exercise fresh-install and cached
   behavior in Zed. For example, run
   `gh attestation verify <asset> --repo chiploom/zed-wit` against downloaded
   bytes. The adapter itself verifies SHA-256, not attestations.

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
