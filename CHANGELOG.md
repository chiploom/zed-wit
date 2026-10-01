# Changelog

## Unreleased

- Replace Python repository and release helpers with locked Rust `xtask` automation,
  including CI enforcement that rejects Python source, tooling/config/cache artifacts,
  shebangs and workflow/script invocations.
- Establish the Rust workspace for the Zed adapter, pinned WIT syntax,
  native analysis and stdio language server.
- Add canonical grammar queries and parser-backed diagnostics with open-buffer
  overlays and bounded package dependency discovery.
- Add Topiary document formatting and regression coverage.
- Define five-platform native CI and a protected manual release workflow with
  SHA-256 sidecars and artifact attestations.
- Document licensing, architecture, development testing and registry succession
  gates. No hosted release or registry publication is claimed by this entry.
