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

Run from the repository root:

```sh
rustup show active-toolchain
cargo xtask check-no-python
cargo xtask check-dependencies
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo check -p zed-wit --target wasm32-wasip2 --locked
cargo build -p wit-language-server --release --locked
cargo xtask package-release --target <target> --output dist
```

Only the adapter is compiled to Wasm. Native tests cover semantic analysis,
formatting, query contracts and the stdio protocol. CI runs native tests and
release builds on all five [distribution targets](docs/publishing.md). A local
host run does not validate the other platforms.

Use [manual testing](docs/manual-testing.md) for Zed development installation and
record the exact Zed version, server path, scenarios and screenshots/logs. Mark
unperformed checks pending. Preserve stdout for LSP frames; diagnostic logging
belongs on stderr.

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
