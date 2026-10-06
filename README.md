# WIT for Zed

Modern editing support for WebAssembly Interface Types, maintained by Chiploom
and useful independently of any Chiploom project. This extension is **unreleased**
and is not published in the Zed registry.

| Feature                  | Implemented behavior                                                                                                         |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| File recognition         | `.wit` files, WIT Markdown code fences                                                                                       |
| Syntax                   | Canonical Bytecode Alliance Tree-sitter grammar, immutable revision                                                          |
| Editing                  | Highlighting, doc/comments, brackets, indentation, outline, Vim text objects                                                 |
| Snippets                 | Package, interface, world, types, functions, imports, exports and use                                                        |
| Diagnostics              | Upstream `wit-parser` syntax, name/type and package errors; unsaved buffers                                                  |
| Packages                 | Sibling multi-file packages, direct `deps/` files and package directories                                                    |
| Formatting               | Maintained Topiary engine; comment-preserving document formatting                                                            |
| Semantic editor features | Scope-aware type completion, resolved hover, go to definition and references; deterministic unresolved type typo quick fixes |

## Install locally

Install [Rust via rustup](https://rustup.rs/), clone this repository, and build:

```sh
cargo build -p wit-language-server --release --locked
./target/release/wit-language-server --version
mkdir -p .zed
cp .zed/settings.example.json .zed/settings.json
```

The project-local settings example points Zed at
`target/release/wit-language-server`, relative to the repository worktree. On
Windows, change the path in the untracked `.zed/settings.json` to
`target/release/wit-language-server.exe`.

The version output includes the Git commit captured when the binary was built,
for example `0.1.0+git.<commit>`. The same build version is exposed through
LSP `serverInfo.version`, which Zed displays in its language-server menu, and
is emitted once as an INFO `window/logMessage` notification so it also appears
in Zed's language-server **View Logs** output.

If you configure the binary in global Zed settings instead of project settings,
use an absolute path to the release executable.

Run `zed: install dev extension` from Zed's command palette and select the
repository root. Zed builds the root adapter package for `wasm32-wasip2` and
downloads WASI SDK separately to compile the grammar. An existing registry WIT
extension will be overridden.
Open a `.wit` file, then use the outline panel and `editor: format` to exercise
the extension. Inspect `zed: open log` for startup or build failures.

The adapter uses an explicitly configured binary first, then
`wit-language-server` on the worktree's PATH. Otherwise it downloads the exact
server version paired with this extension, verifies SHA-256, and caches it in
Zed's extension working directory. **Automatic downloads require a published
release; none exists yet.** Usage after publication will not require Rust/Cargo.
Configured paths, arguments and environment use Zed's normal `lsp` binary settings.

## Compatibility

The audit snapshot is dated 2026-10-01: Rust 1.99.0, Zed extension API 0.7.0,
Tree-sitter ABI 15 and `wit-parser` 0.260.0. See
[upstream compatibility](docs/upstream-compatibility.md) for revisions and sources.
The adapter/native release workflow targets macOS ARM64/x86_64, Linux GNU
ARM64/x86_64 and Windows x86_64 MSVC. Local qualification and remaining platform
checks are recorded in [validation](docs/validation.md).

The grammar recognizes async functions, futures, streams, maps, fixed-length
lists, nested packages, versioned paths, and feature annotations. Current
Component Model milestones include async in 0.3.0 and maps/external IDs in 0.3.1;
fixed-length lists and getter/setter sugar remain gated in the audited spec.
Highlighting recognizes syntax independently of semantic feature gates.
The semantic parser uses its default gates; highlighting does not enable them.
The grammar accepts some obsolete named-result syntax that the semantic parser
rejects, and does not represent gated getter/setter sugar.

## Package behavior and limitations

Open the WIT package directory or its containing project. Sibling `.wit` files
form one package; dependency identities come from WIT declarations, not directory
names. Files and one-level package directories under `deps/` resolve together in
dependency order. Unsaved dependency and sibling edits participate in diagnostics.
The server reports upstream errors at their source file, including closed sibling
files, and clears stale diagnostics after repair.

The server supports local file URIs and full document synchronization. It does
not download dependencies, discover remote packages or recursively crawl the
workspace. Encoded binary component dependencies are not supported in this
initial source-package loader. Formatting refuses trees with syntax errors and
checks idempotence. It formats the current document, preserving multi-file package
boundaries, rather than printing an entire resolved package.

## Development and publication

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo check --workspace --locked
cargo build --target wasm32-wasip2 --locked
```

Read [CONTRIBUTING](CONTRIBUTING.md) for fixtures, queries, updates and releases,
[architecture](docs/architecture.md) for ownership and decisions, and
[manual testing](docs/manual-testing.md) for editor qualification.

The registry already has a `wit` extension. Publishing this implementation as a
successor requires coordination with its maintainer and Zed under the
[replacement policy](docs/publishing.md); no registry PR has been opened.

## Acknowledgements and license

Built on [Bytecode Alliance's grammar](https://github.com/bytecodealliance/tree-sitter-wit),
[wit-parser](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wit-parser),
and [Topiary](https://github.com/topiary/topiary). The
[Bytecode Alliance VS Code extension](https://github.com/bytecodealliance/vscode-wit)
informed behavior; its editor-specific implementation was not ported.

Project-owned code is MIT OR Apache-2.0. See [LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE) and [third-party notices](THIRD_PARTY_NOTICES.md).
Report vulnerabilities through [SECURITY](SECURITY.md).
