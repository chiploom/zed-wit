# Changelog

## Unreleased

- Update public documentation and security guidance for the published `v0.1.0`
  release.
- Add post-release hosted-install qualification for first download, verified cache
  reuse, and corrupt/missing cache recovery in isolated Zed profiles.
- Replace stale pre-release download failure guidance with recovery instructions
  that remain valid after publication.
- Require version-specific user-facing release notes for future CD publications.

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
