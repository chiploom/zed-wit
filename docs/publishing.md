# Publishing and maintenance

The project is unreleased. Defining a workflow does not create a GitHub release,
configure repository protections, or grant permission to replace a registry entry.

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
a newline, and a `<asset>.provenance.json` build record. Build provenance JSON is
not itself a cryptographic attestation.

## Release gate

1. Configure protected default-branch reviews and `v*` tag rules. Configure the
   GitHub `release` environment with required reviewers, prevent self-review and
   restrict deployment branches to the default branch. Enable private security
   reporting. These are manual repository settings; inspect them before release.
2. Complete CI on the intended commit, including all five native targets and the
   adapter Wasm check. Complete and retain the [manual matrix](manual-testing.md).
   Run `cargo xtask check-dependencies`, audit the locked dependency licenses, and
   use `cargo xtask collect-licenses --target <target> --output dist` when preparing
   redistribution notices for a native target.
3. Ensure `Cargo.toml`, the native server manifest, `extension.toml`, and the
   adapter's pinned release version agree. Update changelog and compatibility
   notes. Retain `Cargo.lock`; do not resolve new dependencies during release.
4. Create the immutable stable `vX.Y.Z` tag at that default-branch commit. Dispatch
   **Release native server** from the default branch with that tag. The workflow
   rejects other dispatch branches, mismatched versions and tags pointing to a
   different commit. Prerelease/build-metadata tags are intentionally excluded.
5. Review the build jobs and approve the protected environment. The publisher
   checks the complete asset/checksum set, generates GitHub artifact attestations,
   checks the tag still resolves to the validated commit, and creates the release.
   It never overwrites existing release assets. Attach the license/notice files.
6. Download each published asset and sidecar, verify checksum and provenance, and
   exercise fresh-install and cache behavior in Zed before announcing support.
   For example, run `gh attestation verify <asset> --repo chiploom/zed-wit` against
   the downloaded bytes. The adapter itself verifies SHA-256, not attestations.

Checksums and executable bytes come from the same GitHub origin. They detect
corruption and asset mismatch, not a compromised publishing account. Immutable
action pins, restricted publishing permission, approvals and provenance reduce
separate supply-chain risks. Consult GitHub's
[artifact attestation documentation](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations).

A failed run is not a release. Fix the cause and rerun the full relevant matrix;
do not publish a subset of platforms. Never silently replace assets under a tag.
For a bad published release, document the issue and publish a corrected version;
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
