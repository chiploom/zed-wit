use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("xtask must live at <repo>/crates/xtask")
        .to_path_buf()
}

pub fn root_relative(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        repo_root().join(path)
    }
}

/// Ask Cargo to resolve build.target-dir, including environment variables and
/// the complete configuration hierarchy. Never guess the effective path.
pub fn cargo_target_dir() -> Result<PathBuf, String> {
    let root = repo_root();
    let metadata = command_output(
        "cargo",
        ["metadata", "--format-version", "1", "--no-deps", "--locked"],
        &root,
    )?;
    cargo_target_dir_from_metadata(&metadata)
}

fn cargo_target_dir_from_metadata(metadata: &str) -> Result<PathBuf, String> {
    let value: serde_json::Value = serde_json::from_str(metadata)
        .map_err(|error| format!("parse Cargo metadata: {error}"))?;
    let directory = value
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Cargo metadata omitted target_directory".to_owned())?;
    let target = PathBuf::from(directory);
    if !target.is_absolute() {
        return Err("Cargo metadata target_directory must be absolute".into());
    }
    Ok(target)
}

pub fn command_output<I, S>(program: &str, args: I, cwd: &Path) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("run {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{program} exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim_end().to_owned())
        .map_err(|error| format!("{program} returned non-UTF-8 output: {error}"))
}

pub fn read_nonempty(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.is_empty() {
        return Err(format!("missing or empty text: {}", path.display()));
    }
    String::from_utf8(bytes).map_err(|error| format!("decode {} as UTF-8: {error}", path.display()))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(encode_hex(&hasher.finalize()))
}

pub fn write_new(path: &Path, content: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("create {}: {error}", path.display()))?;
    file.write_all(content.as_bytes())
        .map_err(|error| format!("write {}: {error}", path.display()))
}

pub fn parse_options(
    args: &[String],
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let raw = &args[index];
        let key = raw
            .strip_prefix("--")
            .ok_or_else(|| format!("expected an option, found {raw:?}"))?;
        if !allowed.contains(&key) {
            return Err(format!("unknown option --{key}"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for --{key}"))?;
        if value.starts_with("--") {
            return Err(format!("missing value for --{key}"));
        }
        if options.insert(key.to_owned(), value.clone()).is_some() {
            return Err(format!("duplicate option --{key}"));
        }
        index += 2;
    }
    Ok(options)
}

pub fn required_option(
    options: &mut BTreeMap<String, String>,
    name: &str,
) -> Result<String, String> {
    options
        .remove(name)
        .ok_or_else(|| format!("missing required option --{name}"))
}

pub fn ensure_empty_options(options: BTreeMap<String, String>) -> Result<(), String> {
    if options.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unhandled options: {}",
            options
                .keys()
                .map(|key| format!("--{key}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_target_dir_parser_rejects_incomplete_or_relative_metadata() {
        let absolute = std::env::temp_dir().join("zed-wit-metadata-output");
        let sample = serde_json::json!({"target_directory": absolute});
        assert_eq!(
            cargo_target_dir_from_metadata(&sample.to_string()).unwrap(),
            absolute
        );
        assert!(cargo_target_dir_from_metadata("{}").is_err());
        assert!(
            cargo_target_dir_from_metadata(r#"{"target_directory":"relative"}"#).is_err()
        );
    }

    #[test]
    fn cargo_target_dir_uses_real_cargo_configuration_in_disposable_workspace() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let candidate = std::env::temp_dir().join(format!(
                "zed-wit-metadata-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create Cargo metadata fixture: {error}"),
            }
        };
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove Cargo metadata fixture");
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join(".cargo")).unwrap();
        fs::create_dir(root.join("cargo-home")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"xtask-metadata-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();

        let inspect = |configured: Option<(&str, &Path)>| {
            let mut cmd = Command::new("cargo");
            cmd.args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
                .current_dir(&root)
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("CARGO_BUILD_TARGET_DIR")
                .env("CARGO_HOME", root.join("cargo-home"));
            if let Some((name, value)) = configured {
                cmd.env(name, value);
            }
            let output = cmd.output().expect("execute Cargo metadata");
            assert!(
                output.status.success(),
                "Cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            cargo_target_dir_from_metadata(&String::from_utf8(output.stdout).unwrap()).unwrap()
        };

        assert_eq!(inspect(None), root.join("target"));
        assert_eq!(
            inspect(Some(("CARGO_TARGET_DIR", Path::new("env-target")))),
            root.join("env-target")
        );
        assert_eq!(
            inspect(Some(("CARGO_BUILD_TARGET_DIR", Path::new("build-env-target")))),
            root.join("build-env-target")
        );
        let absolute = root.join("external-absolute");
        assert_eq!(
            inspect(Some(("CARGO_BUILD_TARGET_DIR", &absolute))),
            absolute
        );
        fs::write(
            root.join(".cargo/config.toml"),
            "[build]\ntarget-dir = \"configured-target\"\n",
        )
        .unwrap();
        assert_eq!(inspect(None), root.join("configured-target"));
        assert_eq!(
            inspect(Some(("CARGO_TARGET_DIR", Path::new("env-override")))),
            root.join("env-override")
        );
    }

    #[test]
    fn sha256_hex_encoding_is_canonical() {
        let digest = Sha256::digest(b"abc");
        assert_eq!(
            encode_hex(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn option_parser_rejects_duplicates_and_unknowns() {
        assert!(parse_options(&["--other".into(), "x".into()], &["target"]).is_err());
        assert!(
            parse_options(
                &["--target".into(), "a".into(), "--target".into(), "b".into(),],
                &["target"],
            )
            .is_err()
        );
    }

    #[test]
    fn option_parser_accepts_known_values() {
        let options = parse_options(
            &["--target".into(), "x86_64-unknown-linux-gnu".into()],
            &["target"],
        )
        .unwrap();
        assert_eq!(
            options.get("target").map(String::as_str),
            Some("x86_64-unknown-linux-gnu")
        );
    }
}
