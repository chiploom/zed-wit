# Upstream compatibility

Audit date: 2026-10-07. These observations separate this repository's immutable
pins from the latest upstream revisions checked; they do not claim that hosted CI,
registry succession or every Zed UI/platform test has passed.

| Component                     | Repository pin / published dependency                                                                                                                                      | Latest upstream revision or release checked                                                                                                                   | Decision                                                                                       |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| WIT specification             | No repository dependency pin                                                                                                                                               | [`a25fc0b372dd21f07f0242c46e98bd0f1ea0c0e1`](https://github.com/WebAssembly/component-model/tree/a25fc0b372dd21f07f0242c46e98bd0f1ea0c0e1)                    | Track current syntax; distinguish proposals and feature gates.                                 |
| Canonical WIT grammar         | Repository pin [`f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`](https://github.com/bytecodealliance/tree-sitter-wit/tree/f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6); ABI 15     | Upstream main [`f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`](https://github.com/bytecodealliance/tree-sitter-wit/tree/f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6) | Adopt after requalification; upstream supertype changes preserve the concrete query surface.   |
| Zed source and extension API  | Published `zed_extension_api` 0.7.0 (latest published)                                                                                                                     | Zed source [`6ec43d631972c29422e1915bd6fd597bffa5a88d`](https://github.com/zed-industries/zed/tree/6ec43d631972c29422e1915bd6fd597bffa5a88d)                  | Audited Tree-sitter accepts ABI 13–15; source API 0.8.0 is unpublished.                        |
| Rust                          | Repository toolchain pin 1.99.0                                                                                                                                            | [Rust 1.99.0 channel manifest](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml)                                                                    | Exact toolchain pin in `rust-toolchain.toml`.                                                  |
| Semantic parser               | Published and pinned `wit-parser` 0.260.0                                                                                                                                  | wasm-tools main [`9e3a53fd6cfc59bbd5d56781f66e58c768bbaf1b`](https://github.com/bytecodealliance/wasm-tools/tree/9e3a53fd6cfc59bbd5d56781f66e58c768bbaf1b)    | Keep `wit-parser` as semantic authority; source audit does not update the package pin.         |
| Formatter                     | Pinned `topiary-core` 0.7.3; latest published release 0.8.0                                                                                                                | Topiary main [`78c2c4e938f68ac62255289ae03bb29c93095117`](https://github.com/topiary/topiary/tree/78c2c4e938f68ac62255289ae03bb29c93095117)                   | Do not conflate the published formatter dependency with upstream main; retain tested pin.      |
| VS Code WIT reference         | No repository dependency pin                                                                                                                                               | [`acd5b04d8c9c301a1c20bf53e7b768ad006dcd61`](https://github.com/bytecodealliance/vscode-wit/tree/acd5b04d8c9c301a1c20bf53e7b768ad006dcd61)                    | Uses parser 0.260; reference behavior only. Regex formatting is not reused.                    |
| Registry extension            | Registry `wit` 0.4.0 points to the existing repository                                                                                                                     | Registry tree points at [`7a0c864b748c0a4931c68beaad10236eda74983f`](https://github.com/valentinegb/zed-wit/tree/7a0c864b748c0a4931c68beaad10236eda74983f)    | Replacement is a separate policy gate.                                                         |
| Existing extension repository | No dependency pin; last observed commit [`b4f20086b45da2edaed9e56965d9583410b468c4`](https://github.com/valentinegb/zed-wit/tree/b4f20086b45da2edaed9e56965d9583410b468c4) | Current main remains at the same October 2024 commit                                                                                                          | Age alone grants no transfer rights.                                                           |

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

The semantic surface is diagnostics, document formatting, completion, hover,
definition, references and safe unresolved-type typo quick fixes. Tree-sitter
outline remains distinct from LSP navigation; resolved identities come from
`wit-parser`, while syntax nodes locate source tokens. Rename and workspace symbols
are not advertised. Formatting must preserve comments and file boundaries, be
idempotent, and refuse syntax-error trees. New parser syntax does not automatically
imply formatter support.

Historical [`Michael-F-Bryan/wit-lsp`](https://github.com/Michael-F-Bryan/wit-lsp)
last changed in 2024 and has no audited native release assets.
[`witcraft-lsp`](https://crates.io/crates/witcraft-lsp/0.1.0) 0.1.0 uses a different
parser and has no audited published native assets. Neither provides the required
maintained, distributable backend contract for this implementation.

## 2026-10-07 grammar requalification

Previous pin: [`cdf07263b136054b413cab449ac7a1d059c27542`](https://github.com/bytecodealliance/tree-sitter-wit/tree/cdf07263b136054b413cab449ac7a1d059c27542).
Qualified pin: [`f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`](https://github.com/bytecodealliance/tree-sitter-wit/tree/f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6).

The live upstream history contained exactly two grammar-affecting commits after the
previous pin:

- [`efd516f80394ad24bbf5c7573e492394cef9d1f3`](https://github.com/bytecodealliance/tree-sitter-wit/commit/efd516f80394ad24bbf5c7573e492394cef9d1f3)
  makes `typedef_item` public and declares it as a supertype.
- [`f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6`](https://github.com/bytecodealliance/tree-sitter-wit/commit/f777cdbe11281ccc68ffa30bd7ea34cdf4ddbec6)
  exposes `gate_item`, `statement`, `world_definition`, and `package_items`
  as supertypes and renames the corresponding private wrapper rules.

The generated parser remains ABI 15. The concrete declaration nodes, fields, and
tokens used by this repository's Zed queries are unchanged, so
`highlights.scm`, `indents.scm`, `outline.scm`, `textobjects.scm`,
`overrides.scm`, and `brackets.scm` require no compatibility edits. A native
query regression now proves that each newly public supertype can match its
expected concrete subtype while the existing query suite still compiles against
the exact pinned parser.

Getter/setter sugar remains outside the upstream grammar at this revision. Its
fixture must still produce a Tree-sitter error tree, and the narrowly scoped
highlight recovery remains required. No semantic-parser, formatter, LSP, or
feature-gate behavior is changed by this grammar requalification.

Automated and real-Zed qualification must be rerun on the resulting branch before
merge; this section does not treat unexecuted commands as passing evidence.

## Requalification

When changing a pin, inspect upstream releases and source licenses, then rerun
query compilation/capture fixtures, current and gated syntax, multi-file overlays,
dependency failures, diagnostic clearing, Unicode positions and formatting
preservation/idempotence. Run the full native matrix and adapter Wasm check.
Repeat the [manual Zed scenarios](manual-testing.md) on the exact editor version.
Record actual artifacts; an upstream source audit is not a runtime pass.
