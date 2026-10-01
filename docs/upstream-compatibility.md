# Upstream compatibility

Audit date: 2026-10-01. These are pinned source observations, not a claim that
hosted CI, registry succession or every Zed UI/platform test has passed.

| Component | Audited revision/version | Decision |
| --- | --- | --- |
| WIT specification | [`a25fc0b372dd21f07f0242c46e98bd0f1ea0c0e1`](https://github.com/WebAssembly/component-model/tree/a25fc0b372dd21f07f0242c46e98bd0f1ea0c0e1) | Track current syntax; distinguish proposals and feature gates. |
| Canonical WIT grammar | [`cdf07263b136054b413cab449ac7a1d059c27542`](https://github.com/bytecodealliance/tree-sitter-wit/tree/cdf07263b136054b413cab449ac7a1d059c27542) | Pin Zed grammar and embedded formatter parser together; ABI 15. |
| Zed source | [`cd4fc8de4ca8548cca2567352b87bcaaec13328f`](https://github.com/zed-industries/zed/tree/cd4fc8de4ca8548cca2567352b87bcaaec13328f) | Audited Tree-sitter accepts ABI 13–15. Source API 0.8.0 has `publish = false`. |
| Zed extension API | [`0.7.0`](https://crates.io/crates/zed_extension_api/0.7.0) | Use latest published audited version, not unpublished main-branch API. Adapter target is `wasm32-wasip2`. |
| Rust | [`1.99.0`](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml) | Exact toolchain pin in `rust-toolchain.toml`. |
| Semantic parser | [`wit-parser` 0.260.0 / `3c92a0566d136333757e7c6da680178ae1d91dca`](https://github.com/bytecodealliance/wasm-tools/tree/3c92a0566d136333757e7c6da680178ae1d91dca/crates/wit-parser) | In-memory source maps, structured spans and dependency group resolution. |
| Formatter | [`topiary-core` 0.7.3 / `75ce8324ebaef45e00a964f110ed18ca3ed80235`](https://github.com/topiary/topiary/tree/75ce8324ebaef45e00a964f110ed18ca3ed80235) | Published MIT formatter and WIT queries; preserve comments and file boundaries. |
| Topiary main | [`96b6f425643628cec9b36fb287f15c61c9d05ccd`](https://github.com/topiary/topiary/tree/96b6f425643628cec9b36fb287f15c61c9d05ccd) | Canonical WIT grammar configuration is a reference; do not conflate with the published crate revision. |
| VS Code WIT reference | [`acd5b04d8c9c301a1c20bf53e7b768ad006dcd61`](https://github.com/bytecodealliance/vscode-wit/tree/acd5b04d8c9c301a1c20bf53e7b768ad006dcd61) | Uses parser 0.260; reference behavior only. Regex formatting is not reused. |
| Registry extension | [`wit` 0.4.0](https://github.com/zed-industries/extensions/tree/main/extensions/wit) | Registered to `valentinegb/zed-wit`; replacement is a separate policy gate. |
| Existing extension repository | [`b4f20086b45da2edaed9e56965d9583410b468c4`](https://github.com/valentinegb/zed-wit/tree/b4f20086b45da2edaed9e56965d9583410b468c4) | Last audited change October 2024; age alone grants no transfer rights. |

## Syntax and capability boundaries

Tree-sitter supplies highlighting, brackets, outline and editing structure.
The pinned grammar represents async, future, stream, map, nested packages and
feature annotations. It can also accept legacy named-result syntax rejected by
the current semantic parser. Getter/setter sugar is a gated proposal not fully
represented by this grammar. Syntactic highlighting is not semantic acceptance.
Keep positive and negative fixtures for these differences; do not globally enable
experimental parser gates to make one fixture pass.

`wit-parser` defines package/type/name validity. Package discovery uses sibling
WIT files and direct `deps/` files/package directories, with unsaved document
overlays. It is not a dependency downloader or a recursive workspace crawler.
Structured source spans must remain attached to the right file and converted to
the negotiated LSP position encoding.

The initial semantic surface is diagnostics and document formatting. Tree-sitter
outline is distinct from LSP semantic navigation. Do not advertise completion,
rename, references, or go-to-definition unless their complete implementation and
resolution-aware regression tests are present. Formatting must preserve comments
and file boundaries, be idempotent, and refuse syntax-error trees. New parser
syntax does not automatically imply formatter support.

Historical [`Michael-F-Bryan/wit-lsp`](https://github.com/Michael-F-Bryan/wit-lsp)
last changed in 2024 and has no audited native release assets.
[`witcraft-lsp`](https://crates.io/crates/witcraft-lsp/0.1.0) 0.1.0 uses a different
parser and has no audited published native assets. Neither provides the required
maintained, distributable backend contract for this implementation.

## Requalification

When changing a pin, inspect upstream releases and source licenses, then rerun
query compilation/capture fixtures, current and gated syntax, multi-file overlays,
dependency failures, diagnostic clearing, Unicode positions and formatting
preservation/idempotence. Run the full native matrix and adapter Wasm check.
Repeat the [manual Zed scenarios](manual-testing.md) on the exact editor version.
Record actual artifacts; an upstream source audit is not a runtime pass.
