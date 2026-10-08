//! Single command dispatcher and command-help registry.
use crate::{
    dependency_policy, licenses, release, repository_policy, tasks, util, zed_gui, zed_hosted,
    zed_smoke,
};
use std::{env, path::PathBuf, time::Duration};

pub(crate) const COMMAND_HELP: &[(&str, &str)] = &[
    ("check-dependencies", "[--target <target>]"),
    ("collect-licenses", "--target <target> [--output <dir>]"),
    ("package-release", "--target <target> [--output <dir>]"),
    ("validate-release", "--tag <tag> --scope <lsp|extension>"),
    ("verify-release-assets", "[--input <dir>]"),
    ("check-no-python", ""),
    (
        "test-zed",
        "[--zed <binary>] [--profile <target-subdir>] [--timeout-seconds <5-180>]",
    ),
    (
        "test-zed-hosted",
        "[--zed <binary>] [--profile <target-subdir>] [--timeout-seconds <10-300>]",
    ),
    (
        "test-zed-gui",
        "[--zed <binary>] [--profile <target-subdir>] [--timeout-seconds <5-180>] [--settle-milliseconds <100-5000>] [--linux-input-backend <auto|x11|wayland|libei>] --allow-input-injection true",
    ),
    (
        "release-build",
        "[--target <release-target>] [--output <binary-path>]",
    ),
    (
        "release",
        "--scope <lsp|extension> --tag <tag> [--target <release-target>] [--output <dir>]",
    ),
    ("verify", ""),
    (
        "test",
        "[--package <name>] [--filter <substring>] [--runner <auto|cargo|nextest>]",
    ),
    (
        "build",
        "[--kind <server|extension>] [--release <true|false>] [--target <triple>]",
    ),
    ("check", ""),
    ("dev", ""),
    ("test-all", "[--with-zed <true|false>]"),
    ("release-check", "--scope <lsp|extension> --tag <tag>"),
    ("doctor", ""),
    (
        "clean",
        "--scope <dist|profiles|coverage|build|all> [--execute <true|false>]",
    ),
    ("test-lsp", ""),
    ("test-extension", ""),
    (
        "install-dev",
        "--destination <binary-path> [--target <release-target>]",
    ),
    ("bench", "[--iterations <positive-integer>]"),
    ("coverage", "[--output <path-under-target>]"),
    ("changelog-check", "[--scope <lsp|extension> --tag <tag>]"),
    ("update-grammar", "[--candidate <40-character-git-sha>]"),
    (
        "publish",
        "--scope <lsp|extension> [--bump <patch|minor|major>] [--dry-run|--prepare|--submit|--resume] [--confirm] [--pr <number>] [--wait]",
    ),
];

pub(crate) fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    if matches!(command, "-h" | "--help" | "help") {
        print_help();
        return Ok(());
    }
    dispatch(command, &args[1..])
}

pub(crate) fn dispatch(command: &str, rest: &[String]) -> Result<(), String> {
    if matches!(rest, [flag] if flag == "--help" || flag == "-h")
        && let Some((_, options)) = COMMAND_HELP.iter().find(|(name, _)| *name == command)
    {
        println!("Usage: cargo xtask {command} {options}");
        if matches!(command, "release" | "release-build" | "release-check" | "publish") {
            println!("Local preparation only: protected CD controls publication.");
        }
        if command == "update-grammar" {
            println!("Read-only grammar compatibility audit: never changes the pin.");
        }
        return Ok(());
    }
    match command {
        "check-dependencies" => {
            let mut options = util::parse_options(rest, &["target"])?;
            let target = options.remove("target");
            util::ensure_empty_options(options)?;
            if let Some(target) = target.as_deref() {
                dependency_policy::ensure_target(target)?;
            }
            dependency_policy::run(target.as_deref())
        }
        "collect-licenses" => {
            let mut options = util::parse_options(rest, &["target", "output"])?;
            let target = util::required_option(&mut options, "target")?;
            dependency_policy::ensure_target(&target)?;
            let output = util::root_relative(
                options
                    .remove("output")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("dist")),
            );
            util::ensure_empty_options(options)?;
            licenses::run(&target, &output)
        }
        "package-release" => {
            let mut options = util::parse_options(rest, &["target", "output"])?;
            let target = util::required_option(&mut options, "target")?;
            dependency_policy::ensure_target(&target)?;
            let output = util::root_relative(
                options
                    .remove("output")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("dist")),
            );
            util::ensure_empty_options(options)?;
            release::package_release(&target, &output)
        }
        "validate-release" => {
            let mut options = util::parse_options(rest, &["tag", "scope"])?;
            let tag = util::required_option(&mut options, "tag")?;
            let scope = util::required_option(&mut options, "scope")?;
            util::ensure_empty_options(options)?;
            release::validate_release(&tag, &scope)
        }
        "verify-release-assets" => {
            let mut options = util::parse_options(rest, &["input"])?;
            let input = util::root_relative(
                options
                    .remove("input")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("dist")),
            );
            util::ensure_empty_options(options)?;
            release::verify_release_assets(&input)
        }
        "check-no-python" => {
            if !rest.is_empty() {
                return Err("check-no-python accepts no arguments".into());
            }
            repository_policy::check_no_python()
        }
        "test-zed-gui" => {
            let mut options = util::parse_options(
                rest,
                &[
                    "zed",
                    "profile",
                    "timeout-seconds",
                    "settle-milliseconds",
                    "allow-input-injection",
                    "linux-input-backend",
                ],
            )?;
            let zed = options.remove("zed").unwrap_or_else(|| "zed".into());
            let profile = util::root_relative(
                options
                    .remove("profile")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("target/zed-gui")),
            );
            let timeout = options
                .remove("timeout-seconds")
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --timeout-seconds {value:?}: {error}"))
                })
                .transpose()?
                .unwrap_or(60);
            if !(5..=180).contains(&timeout) {
                return Err("--timeout-seconds must be between 5 and 180".into());
            }
            let settle = options
                .remove("settle-milliseconds")
                .map(|value| {
                    value.parse::<u64>().map_err(|error| {
                        format!("invalid --settle-milliseconds {value:?}: {error}")
                    })
                })
                .transpose()?
                .unwrap_or(750);
            if !(100..=5000).contains(&settle) {
                return Err("--settle-milliseconds must be between 100 and 5000".into());
            }
            let allow_input = options
                .remove("allow-input-injection")
                .as_deref()
                .map(str::parse::<bool>)
                .transpose()
                .map_err(|error| format!("invalid --allow-input-injection value: {error}"))?
                .unwrap_or(false);
            let linux_backend = options
                .remove("linux-input-backend")
                .unwrap_or_else(|| "auto".into());
            util::ensure_empty_options(options)?;
            zed_gui::run(
                &zed,
                &profile,
                Duration::from_secs(timeout),
                Duration::from_millis(settle),
                allow_input,
                &linux_backend,
            )
        }
        "test-zed" => {
            let mut options = util::parse_options(rest, &["zed", "profile", "timeout-seconds"])?;
            let zed = options.remove("zed").unwrap_or_else(|| "zed".into());
            let profile = util::root_relative(
                options
                    .remove("profile")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("target/zed-smoke/profile")),
            );
            let timeout = options
                .remove("timeout-seconds")
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --timeout-seconds {value:?}: {error}"))
                })
                .transpose()?
                .unwrap_or(60);
            if !(5..=180).contains(&timeout) {
                return Err("--timeout-seconds must be between 5 and 180".into());
            }
            util::ensure_empty_options(options)?;
            zed_smoke::run(&zed, &profile, Duration::from_secs(timeout))
        }
        "test-zed-hosted" => {
            let mut options = util::parse_options(rest, &["zed", "profile", "timeout-seconds"])?;
            let zed = options.remove("zed").unwrap_or_else(|| "zed".into());
            let profile = util::root_relative(
                options
                    .remove("profile")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("target/zed-hosted/profile")),
            );
            let timeout = options
                .remove("timeout-seconds")
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --timeout-seconds {value:?}: {error}"))
                })
                .transpose()?
                .unwrap_or(90);
            if !(10..=300).contains(&timeout) {
                return Err("--timeout-seconds must be between 10 and 300".into());
            }
            util::ensure_empty_options(options)?;
            zed_hosted::run(&zed, &profile, Duration::from_secs(timeout))
        }
        "release-build" => tasks::build::release_build(rest),
        "release" => tasks::release_ops::prepare_release(rest),
        "verify" => tasks::validation::verify(rest),
        "test" => tasks::validation::test(rest),
        "build" => tasks::build::build(rest),
        "check" => tasks::validation::check(rest),
        "dev" => tasks::build::dev(rest),
        "test-all" => tasks::validation::test_all(rest),
        "release-check" => tasks::release_ops::release_check(rest),
        "doctor" => tasks::maintenance::doctor(rest),
        "clean" => tasks::maintenance::clean(rest),
        "test-lsp" => tasks::validation::test_lsp(rest),
        "test-extension" => tasks::validation::test_extension(rest),
        "install-dev" => tasks::build::install_dev(rest),
        "bench" => tasks::performance::bench(rest),
        "coverage" => tasks::performance::coverage(rest),
        "changelog-check" => tasks::release_ops::changelog_check(rest),
        "update-grammar" => tasks::maintenance::update_grammar(rest),
        "publish" => tasks::publish::publish(rest),
        other => Err(format!(
            "unknown xtask command {other:?}; run `cargo xtask help`"
        )),
    }
}

fn print_help() {
    println!("Repository automation\n\nUsage:");
    for (command, options) in COMMAND_HELP {
        println!("  cargo xtask {command} {options}");
    }
    println!("Publication and protected tag creation remain CD-only actions.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn command_registry_is_unique_and_all_help_paths_work() {
        assert_eq!(COMMAND_HELP.len(), 28);
        let mut seen = BTreeSet::new();
        for (command, _) in COMMAND_HELP {
            assert!(seen.insert(*command), "duplicate command: {command}");
            assert!(dispatch(command, &["--help".into()]).is_ok(), "{command}");
            assert!(dispatch(command, &["-h".into()]).is_ok(), "{command}");
        }
    }

    #[test]
    fn unknown_command_does_not_fall_back_to_any_implicit_action() {
        assert!(dispatch("publish", &[]).is_err());
        assert!(dispatch("missing-command", &[]).is_err());
        assert!(dispatch("publish", &["--help".into()]).is_err());
    }
}
