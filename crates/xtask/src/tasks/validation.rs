//! Quality gates and test orchestration.
use super::{
    build::build,
    common::{bool_option, cargo, finish, no_args, opts, run, tool_available},
};
use crate::{dependency_policy, repository_policy, util};

pub(crate) fn check(args: &[String]) -> Result<(), String> {
    no_args(args, "check")?;
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo(&["check", "--workspace", "--locked"])
}

pub(crate) fn test(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["package", "filter", "runner"])?;
    let package = options.remove("package");
    let filter = options.remove("filter");
    let runner = options.remove("runner").unwrap_or_else(|| "auto".into());
    finish(options)?;
    if !matches!(runner.as_str(), "auto" | "cargo" | "nextest") {
        return Err(format!("unsupported test runner: {runner}"));
    }
    if filter.is_some() && runner == "nextest" {
        return Err("--filter requires the cargo test runner".into());
    }
    let nextest =
        filter.is_none() && runner != "cargo" && tool_available("cargo", &["nextest", "--version"]);
    if runner == "nextest" && !nextest {
        return Err("cargo-nextest not installed; install it or use --runner cargo".into());
    }
    let mut command = if nextest {
        vec!["nextest".to_owned(), "run".to_owned()]
    } else {
        vec!["test".to_owned()]
    };
    command.push("--locked".into());
    if let Some(package) = package {
        command.extend(["-p".to_owned(), package]);
    } else {
        command.push("--workspace".into());
    }
    if let Some(filter) = filter {
        command.push(filter);
    }
    run("cargo", &command)
}

pub(crate) fn test_lsp(args: &[String]) -> Result<(), String> {
    no_args(args, "test-lsp")?;
    for package in ["wit-syntax", "wit-analysis", "wit-language-server"] {
        test(&["--package".into(), package.into()])?;
    }
    Ok(())
}

pub(crate) fn test_extension(args: &[String]) -> Result<(), String> {
    no_args(args, "test-extension")?;
    test(&["--package".into(), "zed-wit".into()])?;
    test(&["--package".into(), "wit-syntax".into()])?;
    build(&["--kind".into(), "extension".into()])
}

pub(crate) fn test_all(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["with-zed"])?;
    let with_zed = bool_option(&mut options, "with-zed", false)?;
    finish(options)?;
    test(&[])?;
    cargo(&[
        "test",
        "--doc",
        "--workspace",
        "--exclude",
        "xtask",
        "--locked",
    ])?;
    build(&["--kind".into(), "extension".into()])?;
    if with_zed {
        crate::zed_smoke::run(
            "zed",
            &util::repo_root().join("target/zed-smoke/profile"),
            std::time::Duration::from_secs(90),
        )?;
    } else {
        eprintln!(
            "SKIPPED: real-Zed smoke/hosted/GUI tests (opt in with --with-zed true; hosted/GUI retain dedicated commands)"
        );
    }
    Ok(())
}

pub(crate) fn verify(args: &[String]) -> Result<(), String> {
    no_args(args, "verify")?;
    repository_policy::check_no_python()?;
    dependency_policy::run(None)?;
    check(&[])?;
    test_all(&[])
}
