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

/// Resolve Cargo's build output location, including its standard environment override.
pub fn cargo_target_dir() -> PathBuf {
    cargo_target_dir_for(
        &repo_root(),
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
    )
}

fn cargo_target_dir_for(root: &Path, override_dir: Option<&OsStr>) -> PathBuf {
    match override_dir {
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        Some(dir) if !dir.is_empty() => root.join(dir),
        _ => root.join("target"),
    }
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
    fn cargo_target_dir_follows_default_relative_and_absolute_overrides() {
        let root = PathBuf::from("/example/project");
        assert_eq!(cargo_target_dir_for(&root, None), root.join("target"));
        assert_eq!(
            cargo_target_dir_for(&root, Some(OsStr::new("local-build"))),
            root.join("local-build")
        );
        let absolute = std::env::temp_dir().join("zed-wit-custom-target");
        assert!(absolute.is_absolute());
        assert_eq!(
            cargo_target_dir_for(&root, Some(absolute.as_os_str())),
            absolute
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
