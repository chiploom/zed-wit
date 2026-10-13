//! Benchmarks and coverage instrumentation.
use super::common::{cargo, finish, opts, tool_available};
use crate::util;
use std::{fs, path::Path};

pub(crate) fn bench(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["iterations"])?;
    let iterations = options
        .remove("iterations")
        .unwrap_or_else(|| "2000".into());
    finish(options)?;
    let count = iterations
        .parse::<u32>()
        .map_err(|e| format!("invalid --iterations {iterations:?}: {e}"))?;
    if !(10..=1_000_000).contains(&count) {
        return Err("--iterations must be between 10 and 1000000".into());
    }
    cargo(&[
        "bench",
        "-p",
        "wit-syntax",
        "--bench",
        "parse",
        "--locked",
        "--",
        "--iterations",
        &iterations,
    ])
}

pub(crate) fn coverage(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["output"])?;
    let value = options
        .remove("output")
        .unwrap_or_else(|| "target/coverage/lcov.info".into());
    finish(options)?;
    let rel = Path::new(&value);
    let suffix = rel
        .strip_prefix("target/coverage")
        .map_err(|_| "coverage output must be beneath target/coverage/".to_owned())?;
    if suffix.components().count() != 1
        || !matches!(
            suffix.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Err("coverage output must be one filename beneath target/coverage/".into());
    }
    if !tool_available("cargo", &["llvm-cov", "--version"]) {
        return Err(
            "cargo-llvm-cov is required; install with cargo install cargo-llvm-cov --locked".into(),
        );
    }
    let dir = util::repo_root().join("target/coverage");
    if util::repo_root().join("target").is_symlink()
        || dir.is_symlink()
        || util::repo_root().join(&value).is_symlink()
    {
        return Err("refusing coverage output through a symlinked build directory or file".into());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    cargo(&[
        "llvm-cov",
        "--workspace",
        "--exclude",
        "xtask",
        "--locked",
        "--lcov",
        "--output-path",
        &value,
    ])
}
