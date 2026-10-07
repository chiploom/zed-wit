# Changelog

## Unreleased

- Prepare WIT Language Server `v0.1.2` against the requalified Bytecode Alliance
  WIT grammar at `f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`; no concrete
  language-server behavior change is intended.
- Requalify the pinned Bytecode Alliance WIT grammar at `f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`, covering its new public supertypes without changing concrete Zed queries.
- Advance the Zed extension runtime LSP pin to the published immutable
  `v0.1.1` recovery release.
- Prepare the `v0.1.1` LSP recovery release after the immutable `v0.1.0`
  GitHub Release was deleted. GitHub permanently reserves the `v0.1.0` tag
  name, so distribution must continue with a new patch version.
- Update public documentation and security guidance for the historical `v0.1.0`
  release.
- Add post-release hosted-install qualification for first download, verified cache
  reuse, and corrupt/missing cache recovery in isolated Zed profiles.
- Replace stale pre-release download failure guidance with recovery instructions
  that remain valid after publication.
- Require version-specific user-facing release notes for future CD publications.
- Add explicit independent `lsp` and `extension` CD release scopes, decouple
  adapter and server versions through an explicit runtime LSP pin, require
  extension releases to reference a complete published immutable LSP release,
  and reject same-tag recovery assumptions for deleted immutable releases.

## 0.1.0 - 2026-10-07

- Replace Python repository and release helpers with locked Rust `xtask` automation,
  including CI enforcement that rejects Python source, tooling/config/cache artifacts,
  shebangs and workflow/script invocations, and confine disposable Zed qualification
  profiles to the repository `target/` tree before recursive cleanup.
- Establish the Rust workspace for the Zed adapter, pinned WIT syntax,
  native analysis and stdio language server.
- Add canonical grammar queries and parser-backed diagnostics with open-buffer
  overlays and bounded package dependency discovery.
- Add Topiary document formatting and regression coverage.
- Add WIT semantic completion, hover, definition, references and safe unresolved
  type typo quick fixes through the native language server.
- Define five-platform native CI and protected CD that validates release
  candidates before tag creation, builds and verifies every native target, emits
  SHA-256 sidecars and redistribution notices, attests artifacts, and publishes
  resumable draft releases behind the `release` environment.
- Run required pull-request CI even for documentation-only changes so protected
  branch status checks always report.
- Document licensing, architecture, development testing and registry succession
  gates.
