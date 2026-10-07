use std::{env, fs, path::PathBuf};

fn package_version(source: &str) -> Option<&str> {
    let mut in_package = false;
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let value = line.strip_prefix("version")?.trim_start();
        let value = value.strip_prefix('=')?.trim();
        return value.strip_prefix('"')?.strip_suffix('"');
    }
    None
}

fn main() {
    let root = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let server_manifest = root.join("crates/wit-language-server/Cargo.toml");
    println!("cargo::rerun-if-changed={}", server_manifest.display());

    let source = fs::read_to_string(&server_manifest)
        .unwrap_or_else(|error| panic!("read {}: {error}", server_manifest.display()));
    let version = package_version(&source)
        .unwrap_or_else(|| panic!("{} omitted [package] version", server_manifest.display()));

    println!("cargo::rustc-env=WIT_LANGUAGE_SERVER_VERSION={version}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_package_version() {
        let source = r#"
[workspace]
version = "9.9.9"

[package]
name = "server"
version = "1.2.3"

[dependencies]
other = "4"
"#;
        assert_eq!(package_version(source), Some("1.2.3"));
    }
}
