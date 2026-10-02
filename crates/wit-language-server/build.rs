use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const BUILD_COMMIT_ENV: &str = "WIT_LANGUAGE_SERVER_BUILD_COMMIT";

fn command_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_commit(value: &str) -> Option<String> {
    let value = value.trim();
    ((7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn track_git_head(root: &Path) {
    let Some(git_dir) = command_output(root, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir);
    for path in [git_dir.join("HEAD"), git_dir.join("logs/HEAD")] {
        if path.exists() {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}

fn main() {
    println!("cargo::rerun-if-env-changed={BUILD_COMMIT_ENV}");

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must set CARGO_MANIFEST_DIR"),
    );
    let repository_root = manifest_dir.join("../..");
    track_git_head(&repository_root);

    let commit = match env::var(BUILD_COMMIT_ENV) {
        Ok(value) => normalize_commit(&value)
            .unwrap_or_else(|| panic!("{BUILD_COMMIT_ENV} must be a hexadecimal Git commit ID")),
        Err(env::VarError::NotUnicode(_)) => {
            panic!("{BUILD_COMMIT_ENV} must contain valid Unicode")
        }
        Err(env::VarError::NotPresent) => command_output(
            &repository_root,
            &["rev-parse", "--verify", "HEAD"],
        )
        .and_then(|value| normalize_commit(&value))
        .unwrap_or_else(|| "unknown".to_owned()),
    };

    println!("cargo::rustc-env={BUILD_COMMIT_ENV}={commit}");
}
