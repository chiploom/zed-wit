# Agent index

This is an independent, general-purpose WIT extension for Zed. Add no
project-specific syntax, semantics, services or dependencies. <!-- user-specified -->

Read [architecture](docs/architecture.md) before changing boundaries and
[CONTRIBUTING](CONTRIBUTING.md) for setup, validation and update workflows.

## Entry points

- `extension.toml`, `languages/wit/`, `snippets/wit.json`: Zed editing support.
- `src/`: Wasm server discovery/download adapter; no WIT analysis here.
- `crates/wit-syntax/`: pinned grammar and query/capture proof.
- `crates/wit-analysis/`: upstream parsing, package overlays and formatting;
  no LSP types. Never create a second semantic parser. <!-- user-specified -->
- `crates/wit-language-server/`: stdio LSP lifecycle and position conversions.
- `.github/workflows/`, `crates/xtask/`: repository policy, validation and release automation.

## Commands

```sh
cargo xtask check-no-python
cargo xtask check-dependencies
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo check --workspace --locked
cargo build --target wasm32-wasip2 --locked
```

The native server is not a Wasm package. Keep `Cargo.lock`; never edit generated
parser/build output. Grammar upgrades require explicit query and fixture review.
Generated grammar Wasm, `extension.wasm`, `target/`, release artifacts and local
logs are ignored. <!-- user-specified -->

## Sources and rules

Use current [Component Model WIT](https://github.com/WebAssembly/component-model),
[canonical grammar](https://github.com/bytecodealliance/tree-sitter-wit),
[Zed docs](https://zed.dev/docs/extensions/languages), and Bytecode Alliance
`wit-parser` as sources of truth. Reaudit compatibility before changing pins;
[snapshot](docs/upstream-compatibility.md) is dated evidence, not perpetual latest.

For library, API, SDK, CLI or cloud-service documentation, use `npx ctx7@latest
library <official-name> "<specific concept>"` first, then `npx ctx7@latest docs
<resolved-id> "<specific concept>"`. At most three commands per question; omit
sensitive data. For quota errors report the failure and suggest Context7 login
or `CONTEXT7_API_KEY`. Do not silently answer from stale knowledge. General
programming and business-logic review do not need Context7. <!-- user-specified -->

Use official Gitmoji intent plus optional scope and message; do not add
Conventional Commit prefixes or use caveman-commit. Do not create `CLAUDE.md`.
<!-- user-specified -->

Do not introduce Python source, scripts, tooling, build steps, workflow invocations,
or runtime dependencies. Repository automation belongs in the Rust `xtask` crate
unless a separately approved language has a clear architectural advantage.
<!-- user-specified -->

Every claimed capability needs observable proof. Keep Unicode, multi-file,
dependency-overlay, diagnostics-clearing and formatting-idempotence tests.
Describe unavailable GUI/hosted/platform checks explicitly. <!-- user-specified -->

Read [SECURITY](SECURITY.md) for downloads/security and
[publishing](docs/publishing.md) before release or registry work. Contact and
replacement prerequisites must be satisfied before a registry PR; no duplicate
extension submission. <!-- user-specified -->
