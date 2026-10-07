# WIT for Zed

WIT adds first-class [WebAssembly Interface Types (WIT)](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md) support to Zed. The extension combines syntax-aware editing with a native language server for diagnostics, formatting, completion, hover, navigation, references, and safe typo fixes.

The protected **v0.1.0** tag identifies the first LSP-only release, but its
immutable GitHub Release was deleted. GitHub permanently reserves tag names used
by immutable releases, so `v0.1.0` cannot be recreated. LSP distribution resumes
with `v0.1.1`. This is still separate from Zed-extension publication, and the
extension is not yet published in the Zed extension registry.

## Features

| Feature                  | Implemented behavior                                                                                                         |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| File recognition         | `.wit` files and WIT Markdown code fences                                                                                   |
| Syntax                   | Canonical Bytecode Alliance Tree-sitter grammar pinned to an immutable revision                                               |
| Editing                  | Highlighting, doc/comments, brackets, indentation, outline, and Vim text objects                                              |
| Snippets                 | Package, interface, world, types, functions, imports, exports, and `use`                                                     |
| Diagnostics              | Upstream `wit-parser` syntax, name/type, and package errors with unsaved-buffer support                                      |
| Packages                 | Sibling multi-file packages, direct `deps/` files, and package directories                                                   |
| Formatting               | Topiary-based document formatting with comment preservation and idempotence checks                                             |
| Semantic editor features | Scope-aware type completion, resolved hover, go to definition and references, plus deterministic unresolved-type typo fixes    |

## Try it as a development extension

Install [Rust via rustup](https://rustup.rs/), clone this repository, and build the native language server:

```sh
cargo build -p wit-language-server --release --locked
./target/release/wit-language-server --version
mkdir -p .zed
cp .zed/settings.example.json .zed/settings.json
```

The project-local settings example points Zed at
`target/release/wit-language-server`, relative to the repository worktree. On
Windows, change the untracked `.zed/settings.json` path to
`target/release/wit-language-server.exe`.

The version output includes the Git commit captured at build time, for example
`0.1.1+git.<commit>`. The same build identity is exposed through LSP
`serverInfo.version` and emitted once through `window/logMessage`.

In Zed, run **zed: install dev extension** from the command palette and select
the repository root. Open a `.wit` file, then use the outline panel,
completion, navigation, diagnostics, and **editor: format** to exercise the
extension. Inspect **zed: open log** for startup or build failures.

An existing registry WIT extension is overridden by the development install.
Automatic native-server downloads follow
`package.metadata.zed-wit.runtime-lsp-version` in the root manifest. The
current pin remains `0.1.0`, whose immutable GitHub Release was deleted, so
hosted downloads remain unavailable until the recovery `v0.1.1` release is
published and a follow-up change advances the pin. The LSP package version can
advance independently without changing what the extension downloads.

## How it works

The root Zed extension is a small Wasm adapter. It uses an explicitly configured
language-server binary first, then `wit-language-server` on the worktree's
`PATH`. Otherwise it downloads the exact native server version paired with the
extension, verifies the SHA-256 sidecar, and caches the executable in Zed's
extension working directory.

The native server uses upstream `wit-parser` semantics rather than maintaining a
second WIT parser. Sibling `.wit` files form a package, dependency identities
come from WIT declarations rather than directory names, and direct files plus
one-level package directories under `deps/` resolve in dependency order.
Unsaved sibling and dependency edits participate in diagnostics.

The server supports local file URIs and full document synchronization. It does
not download package dependencies, discover remote packages, recursively crawl
the workspace, or load encoded binary component dependencies. Formatting is
document-scoped, refuses syntax-invalid trees, preserves comments, and checks
idempotence.

## Compatibility

The current compatibility snapshot records Rust 1.99.0, Zed extension API 0.7.0,
Tree-sitter ABI 15, and `wit-parser` 0.260.0. See
[upstream compatibility](docs/upstream-compatibility.md) for audited revisions
and sources.

The native release matrix targets:

- macOS ARM64 and x86_64;
- Linux GNU ARM64 and x86_64;
- Windows x86_64 MSVC.

The grammar recognizes async functions, futures, streams, maps, fixed-length
lists, nested packages, versioned paths, and feature annotations. Current
Component Model milestones include async in 0.3.0 and maps/external IDs in
0.3.1; fixed-length lists and getter/setter sugar remain gated in the audited
spec. Highlighting recognizes syntax independently of semantic feature gates.
See [validation](docs/validation.md) for qualification evidence and known gaps.

## Development

```sh
cargo xtask check-no-python
cargo xtask check-dependencies
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo check --workspace --locked
cargo build --target wasm32-wasip2 --locked
```

After a matching GitHub release exists, `cargo xtask test-zed-hosted` exercises
the real hosted download, cache reuse, and cache-recovery path in an isolated Zed
profile.

Read [CONTRIBUTING](CONTRIBUTING.md) for setup, fixtures, queries, and validation
workflows; [architecture](docs/architecture.md) for component boundaries; and
[manual testing](docs/manual-testing.md) for real-editor qualification.

## Publication status

The Zed registry already contains a `wit` extension. Publishing this
implementation as its successor requires coordination with the existing
maintainer and Zed under the [replacement policy](docs/publishing.md). No
registry transfer or duplicate entry is assumed.

GitHub release publication is separate from registry publication. Version
`v0.1.0` is the protected **LSP-only** release tag. When its GitHub Release
record exists, repository immutable-release policy protects the published
assets. Future CD runs explicitly choose `lsp` or `extension` scope. LSP and extension versions can advance independently; the
extension pins a published LSP version. See [publishing](docs/publishing.md) for
the scope contract and registry succession gate.

## Acknowledgements and license

Built on [Bytecode Alliance's Tree-sitter WIT grammar](https://github.com/bytecodealliance/tree-sitter-wit),
[wit-parser](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wit-parser),
and [Topiary](https://github.com/topiary/topiary). The
[Bytecode Alliance VS Code extension](https://github.com/bytecodealliance/vscode-wit)
informed expected editor behavior; its editor-specific implementation was not
ported.

Project-owned code is MIT OR Apache-2.0. See [LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE), and
[third-party notices](THIRD_PARTY_NOTICES.md). Report vulnerabilities through
[SECURITY](SECURITY.md).
