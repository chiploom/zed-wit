use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let manifest_path = root.join("Cargo.toml");
    println!("cargo::rerun-if-changed={}", manifest_path.display());

    let source = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display()));
    let manifest: toml::Value = toml::from_str(&source)
        .unwrap_or_else(|error| panic!("parse {}: {error}", manifest_path.display()));
    let version = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("zed-wit"))
        .and_then(|metadata| metadata.get("runtime-lsp-version"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "{} omitted package.metadata.zed-wit.runtime-lsp-version",
                manifest_path.display()
            )
        });

    println!("cargo::rustc-env=WIT_LANGUAGE_SERVER_VERSION={version}");
}
