# Release notes

GitHub releases are scoped explicitly.

- `lsp`: native WIT language-server release only. Attached release assets are
  the five native LSP binaries plus their checksums, provenance, redistribution
  notices, and project license files. This scope does not publish the Zed
  extension.
- `extension`: Zed extension source release only. The protected
  `v-extension-X.Y.Z` tag and GitHub-generated source archive represent the
  extension source, and CD verifies that the extension builds for
  `wasm32-wasip2`. No native LSP binaries are attached. Before publication, CD
  requires the explicit `package.metadata.zed-wit.runtime-lsp-version` pin to
  resolve to a published, non-prerelease, immutable `vX.Y.Z` LSP release with
  every supported runtime binary and checksum. This scope does not publish or
  replace the Zed extension registry entry.

LSP and extension versions are independent. LSP releases use protected
`vX.Y.Z` tags; extension releases use protected `v-extension-X.Y.Z` tags.
Both remain covered by the repository's `v*` release-tag ruleset.

Release notes live at `docs/releases/<scope>/vX.Y.Z.md`. The CD workflow refuses
to publish when the notes file for the selected scope/version is missing or
empty. The workflow prepends the canonical scope notice to the GitHub Release
body, so release-note files should not invent a different scope label.

## Regeneration

Deleted LSP GitHub Releases may be restored only while the original successful
publication run's `release-*` artifacts remain available. CD validates that
source run against the protected tag SHA and restores the original artifact
bytes. It does not rebuild LSP binaries for an existing release tag.

If those Actions artifacts have expired, create a new version rather than
publishing newly built bytes under the old tag.
