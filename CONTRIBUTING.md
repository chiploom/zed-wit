# Contributing

Read [architecture](docs/architecture.md), the [ADRs](docs/adr/), and
[upstream compatibility](docs/upstream-compatibility.md) before changing a layer.
Keep the Zed adapter free of native analysis dependencies. Preserve upstream
licenses and do not introduce capabilities without protocol tests.

## Development

Install Rust through rustup and a native C compiler (Xcode command-line tools on
macOS, a C build toolchain on Linux, or Visual Studio C++ build tools on Windows).
Repository automation is implemented in the Rust `xtask` crate; Python is not
used or permitted in this repository. `rust-toolchain.toml` pins Rust 1.99.0,
rustfmt, clippy and the `wasm32-wasip2` target. Commit `Cargo.lock`.

See the [xtask command reference](docs/xtask.md) for development, release-preparation, tests, diagnostics and maintenance. For version-aware publishing, follow the staged, confirm-only [publish guide](docs/xtask-publish-implementation.md); there is no direct local tag or release creation.

Run from the repository root:

```sh
rustup toolchain install
cargo xtask check-no-python
cargo xtask check-dependencies
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo build --target wasm32-wasip2 --locked
cargo build -p wit-language-server --release --locked
cargo xtask package-release --target <target> --output dist
cargo xtask collect-licenses --target <target> --output dist
```

Only the adapter is compiled to Wasm. Native tests cover semantic analysis,
formatting, query contracts and the stdio protocol. CI runs native tests and
release builds on all five [distribution targets](docs/publishing.md). A local
host run does not validate the other platforms.

Use [manual testing](docs/manual-testing.md) for Zed development installation and
record the exact Zed version, server path, scenarios and screenshots/logs. Mark
unperformed checks pending. Preserve stdout for LSP frames; diagnostic logging
belongs on stderr.

For local Zed development, copy the committed example settings and keep the real
project settings untracked:

```sh
mkdir -p .zed
cp .zed/settings.example.json .zed/settings.json
```

The example points the WIT language server at the repository-local release
binary, `target/release/wit-language-server`. Build it first with:

```sh
cargo build -p wit-language-server --release --locked
```

On Windows, change the project-local binary path to
`target/release/wit-language-server.exe`. Do not commit `.zed/settings.json`;
it is intentionally ignored so platform- or developer-specific Zed settings do
not leak into the shared repository configuration.

## Changes and updates

Keep changes focused, add regression coverage for behavior, and update the
changelog and relevant compatibility notes. Update grammar revision, generated
parser/node metadata, queries and formatting compatibility together. Parser and
formatter updates require current/gated syntax fixtures, multi-file overlays,
dependency diagnostics, Unicode positions and formatting idempotence checks.
Dependency updates are grouped by Dependabot and require human review; no workflow
automatically merges them.

Contributions are licensed under MIT OR Apache-2.0 unless explicitly agreed
otherwise. Retain the distinct licenses and attribution of upstream material.
Do not publish or contact external maintainers as an incidental part of a code
change. Follow the separate [publication gates](docs/publishing.md).
