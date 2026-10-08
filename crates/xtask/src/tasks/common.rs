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

pub(crate) fn bool_option(options: &mut BTreeMap<String, String>, key: &str, default: bool) -> Result<bool, String> {
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
    run("cargo", &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
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

pub(crate) fn native_binary(target: Option<&str>, release_profile: bool) -> PathBuf {
    let mut binary = util::repo_root().join("target");
    if let Some(target) = target {
        binary.push(target);
    }
    binary.push(if release_profile { "release" } else { "debug" });
    binary.push(if target.is_some_and(|value| value.contains("windows")) || (target.is_none() && cfg!(windows)) {
        "wit-language-server.exe"
    } else {
        "wit-language-server"
    });
    binary
}

pub(crate) fn ensure_regular_file(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if !meta.file_type().is_file() || meta.len() == 0 {
        return Err(format!("expected a nonempty regular file: {}", path.display()));
    }
    Ok(())
}

pub(crate) fn copy_new_file(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_regular_file(source)?;
    if source == destination {
        return Err("source and destination are the same".into());
    }
    let mut source_file = File::open(source).map_err(|e| format!("open {}: {e}", source.display()))?;
    let mut dest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("create {} (will not overwrite): {e}", destination.display()))?;
    if let Err(error) = io::copy(&mut source_file, &mut dest) {
        drop(dest);
        let _ = fs::remove_file(destination);
        return Err(format!("copy {}: {error}", destination.display()));
    }
    if let Err(error) = dest.flush().and_then(|()| dest.sync_all()) {
        drop(dest);
        let _ = fs::remove_file(destination);
        return Err(format!("write {}: {error}", destination.display()));
    }
    drop(dest);
    let permissions = fs::metadata(source)
        .map_err(|e| format!("stat {}: {e}", source.display()))?
        .permissions();
    if let Err(error) = fs::set_permissions(destination, permissions) {
        let _ = fs::remove_file(destination);
        return Err(format!("permissions {}: {error}", destination.display()));
    }
    println!("installed {}", destination.display());
    Ok(())
}
