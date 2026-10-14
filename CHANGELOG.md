# Changelog

## Unreleased

### v-extension-0.5.2 (extension)

- Prepare Chiploom's first published Zed WIT extension release, succeeding the original implementation with the previous maintainer's permission.
- Supersede the unpublished `v-extension-0.5.0` and `v-extension-0.5.1` release candidates after their protected CD workflows stopped during tag-policy validation, before creating Git tags or GitHub Releases.
- Provide modern WIT syntax highlighting, editor configuration, snippets, code outlines, and Vim text objects using the pinned Bytecode Alliance Tree-sitter grammar.
- Integrate a native WIT language server providing parser-backed diagnostics, scope-aware completion, hover, definition navigation, references, and safe unresolved-type quick fixes.
- Support multi-file WIT package analysis, unsaved-buffer overlays, and local dependency resolution.
- Provide Topiary-based document formatting with comment preservation, configurable indentation, and idempotence checks.
- Automatically download and verify the published immutable WIT Language Server `v0.1.2`, with SHA-256 integrity checks, executable caching, and recovery from corrupted or incomplete downloads.
- Support native language-server distribution on macOS ARM64/x86_64, Linux GNU ARM64/x86_64, and Windows x86_64.
- Preserve independent extension and native LSP version streams, with the extension runtime pinned to `v0.1.2`.
- Maintain protected, human-reviewed release preparation and publication with exact source-commit verification, immutable-release enforcement, and strict repository policy validation.
- Correct tag-ruleset validation to handle permission-dependent redaction of bypass-actor metadata without weakening the independently audited protection requirements.
- Accept the two verified, equivalent local-offset and UTC representations of the audited ruleset update timestamp while rejecting changed revisions.
- Preserve fail-closed behavior for missing protections, unexpected bypass actors, unauthorized configuration changes, and incomplete release metadata.
- Reconcile temporarily incomplete GitHub Actions run metadata through bounded retries while strictly validating run identity and source commits.
- Expand publishing regression coverage for ruleset timestamp normalization, policy changes, workflow identity mismatches, and retry exhaustion.
- Retain the original extension feature set and document the maintainer transition, supported platforms, known limitations, and acknowledgements to Valentine.

### v-extension-0.5.1 (extension)

- Prepare Chiploom's first published Zed WIT extension release, succeeding the original implementation with the previous maintainer's permission.
- Supersede the unpublished `v-extension-0.5.0` release candidate after its protected CD workflow stopped during tag-protection validation, before creating a tag or GitHub Release.
- Introduce modern WIT syntax highlighting, editor configuration, snippets, outlines, and Vim text objects using the pinned Bytecode Alliance Tree-sitter grammar.
- Integrate a native WIT language server providing parser-backed diagnostics, scope-aware completion, hover, definition navigation, references, and safe unresolved-type quick fixes.
- Add multi-file WIT package analysis with unsaved-buffer support and local dependency resolution.
- Provide Topiary-based document formatting with comment preservation and idempotence checks.
- Automatically download and verify the published WIT Language Server `v0.1.2`, with executable caching and recovery from corrupted or incomplete downloads.
- Support native language-server distribution on macOS ARM64/x86_64, Linux GNU ARM64/x86_64, and Windows x86_64.
- Maintain independent extension and native LSP version streams, with the extension runtime pinned to `v0.1.2`.
- Correct protected CD tag-ruleset validation to handle permission-dependent redaction of bypass-actor metadata while requiring an independently audited ruleset revision.
- Preserve fail-closed release behavior for changed rulesets, missing protections, and explicitly configured bypass actors.
- Add bounded reconciliation for temporarily incomplete GitHub Actions metadata following a confirmed release dispatch, without relaxing workflow identity checks.
- Expand release-policy and workflow-state regression coverage, including protection changes, malformed identity responses, and retry exhaustion.
- Document the maintainer transition and acknowledge Valentine for the original extension.

### v-extension-0.5.0 (extension)

- Prepare the first Chiploom-maintained WIT extension release, succeeding the original Zed registry implementation with the original author's permission.
- Introduce updated WIT syntax highlighting, editor configuration, snippets, outlines, and Vim text objects using the pinned Bytecode Alliance Tree-sitter grammar.
- Integrate a native WIT language server providing diagnostics, scope-aware completion, hover, definition navigation, references, and unresolved-type quick fixes.
- Add multi-file WIT package analysis with unsaved-buffer support and local dependency resolution.
- Provide Topiary-based document formatting with comment preservation and idempotence checks.
- Automatically download and verify the pinned WIT Language Server `v0.1.2`, with caching and recovery from corrupted or incomplete downloads.
- Support native language-server distribution on macOS ARM64/x86_64, Linux GNU ARM64/x86_64, and Windows x86_64.
- Align the extension version with the existing Zed registry's version history while keeping native LSP releases independently versioned.
- Document the maintainer transition and credit Valentine for the original extension.

- Add an opt-in, version-aware Rust xtask publishing frontend with a read-only
  default plan, separate LSP/extension SemVer streams, human-reviewed
  release-preparation PRs, and explicitly confirmed protected CD dispatch.
  Local commands still do not create tags, publish releases or update the Zed
  registry.

- Refactor xtask into a unified CLI registry and focused Rust modules for
  build, validation, release preparation, maintenance and performance, with
  regression checks for all 27 existing and new commands.
- Add 18 Rust xtask developer commands for native and adapter builds, local
  release preparation, validation, Zed testing, environment diagnosis, safe
  cleanup, coverage, grammar-pin auditing, and WIT parsing benchmarks.
  Preserve protected GitHub release publication and Zed registry gates.

- Update the Zed extension runtime LSP pin to the published immutable `0.1.2`
  release, with matching distribution and compatibility documentation. Record
  macOS ARM64 hosted/GUI qualification and track remaining platforms in #22.

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
