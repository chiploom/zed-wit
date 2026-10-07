# Release notes

GitHub releases are scoped explicitly.

- `lsp`: native WIT language-server release only. Attached release assets are
  the five native LSP binaries plus their checksums, provenance, redistribution
  notices, and project license files. This scope does not publish the Zed
  extension.
- `full`: the Git tag represents the full WIT-for-Zed source release together
  with the native LSP assets. GitHub's source archive contains the extension
  source at that tag; attached binary assets remain LSP artifacts. A full GitHub
  release still does not publish or replace the Zed extension registry entry.

Release notes live at `docs/releases/<scope>/vX.Y.Z.md`. The CD workflow refuses
to publish when the notes file for the selected scope and tag is missing or
empty.
