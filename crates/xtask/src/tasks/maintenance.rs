//! Environment diagnostics, bounded cleanup and grammar audits.
use super::common::{bool_option, cargo, finish, host_target, no_args, opts, tool_available};
use crate::{dependency_policy, util};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn doctor(args: &[String]) -> Result<(), String> {
    no_args(args, "doctor")?;
    let root = util::repo_root();
    let mut errors = Vec::new();
    for (name, command) in [
        ("cargo", vec!["--version"]),
        ("rustc", vec!["--version"]),
        ("rustup", vec!["--version"]),
        ("git", vec!["--version"]),
    ] {
        if tool_available(name, &command) {
            println!("OK: {name}");
        } else {
            errors.push(format!(
                "missing {name}; install the Rust toolchain or Git as appropriate"
            ));
        }
    }
    if let Ok(installed) = util::command_output("rustup", ["target", "list", "--installed"], &root)
    {
        if installed.lines().any(|line| line == "wasm32-wasip2") {
            println!("OK: wasm32-wasip2 target");
        } else {
            errors.push("missing wasm32-wasip2 target; run rustup target add wasm32-wasip2".into());
        }
    } else {
        errors.push("cannot inspect installed Rust targets".into());
    }
    let compiler_found = if cfg!(windows) {
        tool_available("where", &["cl"])
    } else {
        tool_available("cc", &["--version"]) || tool_available("clang", &["--version"])
    };
    if compiler_found {
        println!("OK: native C compiler");
    } else {
        errors.push(
            "missing a native C compiler; install Xcode CLT, GCC/Clang, or MSVC Build Tools".into(),
        );
    }
    if let Ok(host) = host_target() {
        println!("Host target: {host}");
        if !dependency_policy::TARGETS.contains(&host.as_str()) {
            println!("NOTE: host is not among the five native release targets");
        }
    }
    for (name, program, args) in [
        ("cargo-nextest", "cargo", vec!["nextest", "--version"]),
        ("cargo-llvm-cov", "cargo", vec!["llvm-cov", "--version"]),
        ("zed", "zed", vec!["--version"]),
    ] {
        if tool_available(program, &args) {
            println!("OPTIONAL OK: {name}");
        } else {
            println!(
                "OPTIONAL UNAVAILABLE: {name}; its specific commands may require installation"
            );
        }
    }
    if errors.is_empty() {
        println!(
            "Required environment checks passed; this does not qualify real-Zed/platform release behavior."
        );
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

pub(crate) fn safe_generated_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    const ALLOWED: &[&str] = &[
        "dist",
        "target/coverage",
        "target/zed-gui",
        "target/zed-hosted",
        "target/zed-smoke",
    ];
    if !ALLOWED.contains(&relative) {
        return Err(format!("cleanup path is not allowlisted: {relative}"));
    }
    if relative.starts_with("target/") {
        let target = root.join("target");
        if let Ok(meta) = fs::symlink_metadata(&target)
            && meta.file_type().is_symlink()
        {
            return Err("refusing cleanup through a symlinked target directory".into());
        }
    }
    let path = root.join(relative);
    if let Ok(meta) = fs::symlink_metadata(&path)
        && (meta.file_type().is_symlink() || !meta.is_dir())
    {
        return Err(format!(
            "refusing cleanup of non-directory or symlink: {}",
            path.display()
        ));
    }
    Ok(path)
}

pub(crate) fn clean(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "execute"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let execute = bool_option(&mut options, "execute", false)?;
    finish(options)?;
    let mut paths = match scope.as_str() {
        "dist" => vec!["dist"],
        "profiles" => vec!["target/zed-smoke", "target/zed-hosted", "target/zed-gui"],
        "coverage" => vec!["target/coverage"],
        "build" => vec![],
        "all" => vec![
            "dist",
            "target/zed-smoke",
            "target/zed-hosted",
            "target/zed-gui",
            "target/coverage",
        ],
        _ => return Err(format!("invalid cleanup scope {scope:?}")),
    };
    let root = util::repo_root();
    let resolved = paths
        .drain(..)
        .map(|p| safe_generated_path(&root, p))
        .collect::<Result<Vec<_>, _>>()?;
    if matches!(scope.as_str(), "build" | "all") && root.join("target").is_symlink() {
        return Err("refusing cargo clean through a symlinked target directory".into());
    }
    if execute && cfg!(windows) && matches!(scope.as_str(), "build" | "all") {
        return Err(
            "cannot remove a running xtask.exe on Windows; run cargo clean --locked directly"
                .into(),
        );
    }
    for path in resolved {
        if execute && path.exists() {
            fs::remove_dir_all(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            println!("removed {}", path.display());
        } else {
            println!(
                "{} {}",
                if execute { "not present:" } else { "dry-run:" },
                path.display()
            );
        }
    }
    if matches!(scope.as_str(), "build" | "all") {
        if execute {
            cargo(&["clean", "--locked"])?;
        } else {
            println!("dry-run: cargo clean --locked");
        }
    }
    if !execute {
        println!("No changes made. Supply --execute true to remove generated data.");
    }
    Ok(())
}

pub(crate) fn update_grammar(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["candidate"])?;
    let candidate = options.remove("candidate");
    finish(options)?;
    let root = util::repo_root();
    let extension: toml::Value =
        toml::from_str(&util::read_nonempty(&root.join("extension.toml"))?)
            .map_err(|e| format!("parse extension.toml: {e}"))?;
    let syntax: toml::Value = toml::from_str(&util::read_nonempty(
        &root.join("crates/wit-syntax/Cargo.toml"),
    )?)
    .map_err(|e| format!("parse wit-syntax Cargo.toml: {e}"))?;
    let ext_rev = extension
        .get("grammars")
        .and_then(|v| v.get("wit"))
        .and_then(|v| v.get("rev"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml has no grammars.wit.rev".to_owned())?;
    let cargo_rev = syntax
        .get("dependencies")
        .and_then(|v| v.get("tree-sitter-wit"))
        .and_then(|v| v.get("rev"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "wit-syntax manifest has no tree-sitter-wit.rev".to_owned())?;
    if ext_rev != cargo_rev {
        return Err(format!(
            "grammar pin mismatch: extension={ext_rev} syntax={cargo_rev}"
        ));
    }
    let notes = util::read_nonempty(&root.join("docs/upstream-compatibility.md"))?;
    if !notes.contains(ext_rev) {
        return Err(
            "upstream-compatibility.md does not document the pinned grammar revision".into(),
        );
    }
    println!("Current pinned grammar: {ext_rev} (extension and syntax manifests agree)");
    if let Some(candidate) = candidate {
        if candidate.len() != 40 || !candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("--candidate requires a 40-character hexadecimal Git commit".into());
        }
        println!("Candidate: {candidate}");
        println!(
            "Review upstream grammar delta, query captures and fixtures, and update both pins, Cargo.lock and dated compatibility evidence together."
        );
    }
    println!(
        "Read-only audit complete. No pin or source changed; see issue #17 for requalification."
    );
    Ok(())
}
