//! Shared command execution and filesystem primitives.
use crate::util;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) fn no_args(args: &[String], name: &str) -> Result<(), String> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!("{name} does not accept arguments; see --help"))
    }
}

pub(crate) fn opts(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>, String> {
    util::parse_options(args, allowed)
}

pub(crate) fn bool_option(
    options: &mut BTreeMap<String, String>,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match options.remove(key).as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(value) => Err(format!("--{key} must be true or false, got {value:?}")),
    }
}

pub(crate) fn finish(options: BTreeMap<String, String>) -> Result<(), String> {
    util::ensure_empty_options(options)
}

pub(crate) fn run(program: &str, args: &[String]) -> Result<(), String> {
    eprintln!("+ {} {}", program, args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(util::repo_root())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("cannot execute {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed with {status}", args.join(" ")))
    }
}

pub(crate) fn cargo(args: &[&str]) -> Result<(), String> {
    run(
        "cargo",
        &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
    )
}

pub(crate) fn tool_available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(util::repo_root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn host_target() -> Result<String, String> {
    let verbose = util::command_output("rustc", ["-vV"], &util::repo_root())?;
    let host = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| "rustc -vV omitted the host target".to_owned())?;
    Ok(host.to_owned())
}

pub(crate) fn native_binary(target: Option<&str>, release_profile: bool) -> Result<PathBuf, String> {
    let mut binary = util::cargo_target_dir()?;
    if let Some(target) = target {
        binary.push(target);
    }
    binary.push(if release_profile { "release" } else { "debug" });
    binary.push(
        if target.is_some_and(|value| value.contains("windows"))
            || (target.is_none() && cfg!(windows))
        {
            "wit-language-server.exe"
        } else {
            "wit-language-server"
        },
    );
    Ok(binary)
}

pub(crate) fn ensure_regular_file(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if !meta.file_type().is_file() || meta.len() == 0 {
        return Err(format!(
            "expected a nonempty regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Copy to a new destination without touching any pre-existing entry.
/// On I/O failure, an incomplete newly created file may remain; never remove
/// by pathname after opening because another process may replace that entry.
pub(crate) fn copy_new_file(source: &Path, destination: &Path) -> Result<(), String> {
    copy_new_file_with(source, destination, io::copy)
}

fn copy_new_file_with<F>(source: &Path, destination: &Path, copier: F) -> Result<(), String>
where
    F: FnOnce(&mut File, &mut File) -> io::Result<u64>,
{
    ensure_regular_file(source)?;
    if source == destination {
        return Err("source and destination are the same".into());
    }
    let mut source_file =
        File::open(source).map_err(|e| format!("open {}: {e}", source.display()))?;
    let source_meta = source_file
        .metadata()
        .map_err(|e| format!("stat open source {}: {e}", source.display()))?;
    if !source_meta.is_file() || source_meta.len() == 0 {
        return Err(format!(
            "expected a nonempty regular file: {}",
            source.display()
        ));
    }
    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("create {} (will not overwrite): {e}", destination.display()))?;

    // Retain the file handle for all writes and permission updates. Never unlink
    // by pathname after a failure: the name might now belong to a different file.
    copier(&mut source_file, &mut dest).map_err(|e| {
        format!(
            "copy {}: {e}; incomplete destination may remain",
            destination.display()
        )
    })?;
    dest.flush().and_then(|()| dest.sync_all()).map_err(|e| {
        format!(
            "write {}: {e}; incomplete destination may remain",
            destination.display()
        )
    })?;
    dest.set_permissions(source_meta.permissions())
        .map_err(|e| {
            format!(
                "permissions {}: {e}; destination may remain",
                destination.display()
            )
        })?;
    println!("installed {}", destination.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let base = std::env::temp_dir();
            loop {
                let name = format!(
                    "zed-wit-xtask-copy-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                );
                let path = base.join(name);
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    #[test]
    fn copy_never_overwrites_existing_file_or_hard_link() {
        let dir = TestDir::new();
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        fs::write(&source, b"original").unwrap();
        fs::write(&destination, b"preserved").unwrap();
        assert!(copy_new_file(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"preserved");
        assert!(copy_new_file(&source, &dir.0.join("./source")).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");

        fs::remove_file(&destination).unwrap();
        fs::hard_link(&source, &destination).unwrap();
        assert!(copy_new_file(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn copy_rejects_existing_symlink_destination_and_symlink_source() {
        use std::os::unix::fs::symlink;
        let dir = TestDir::new();
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        fs::write(&source, b"original").unwrap();
        symlink(&source, &destination).unwrap();
        assert!(copy_new_file(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert!(copy_new_file(&destination, &dir.0.join("copy")).is_err());
    }

    // Windows can deny renaming an open file, so the deterministic swap test
    // runs on Unix; ordinary copy failures remain tested on all platforms.
    #[cfg(unix)]
    #[test]
    fn copy_failure_does_not_unlink_a_replacement_destination() {
        let dir = TestDir::new();
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        let renamed = dir.0.join("renamed");
        fs::write(&source, b"original").unwrap();

        let error = copy_new_file_with(&source, &destination, |_source, opened_dest| {
            opened_dest.write_all(b"partial")?;
            fs::rename(&destination, &renamed)?;
            fs::write(&destination, b"replacement")?;
            Err(io::Error::other("injected copy failure"))
        })
        .unwrap_err();

        assert!(error.contains("injected copy failure"));
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        assert_eq!(fs::read(&renamed).unwrap(), b"partial");
    }

    #[test]
    fn failed_copy_leaves_explicitly_reported_partial_file() {
        let dir = TestDir::new();
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        fs::write(&source, b"original").unwrap();
        let error = copy_new_file_with(&source, &destination, |_source, dest| {
            dest.write_all(b"partial")?;
            Err(io::Error::other("simulated write error"))
        })
        .unwrap_err();
        assert!(error.contains("incomplete destination may remain"));
        assert_eq!(fs::read(&destination).unwrap(), b"partial");
    }

    #[test]
    fn copies_bytes_to_new_file_and_refuses_repeat() {
        let dir = TestDir::new();
        let source = dir.0.join("source");
        let destination = dir.0.join("destination");
        fs::write(&source, b"original").unwrap();
        copy_new_file(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert!(copy_new_file(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"original");
    }
}
