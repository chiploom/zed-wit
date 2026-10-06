mod dependency_policy;
mod licenses;
mod release;
mod repository_policy;
mod util;
mod zed_gui;
mod zed_smoke;

use std::{env, path::PathBuf, process::ExitCode, time::Duration};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    if matches!(command, "-h" | "--help" | "help") {
        print_help();
        return Ok(());
    }

    let rest = &args[1..];
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
            let mut options = util::parse_options(rest, &["tag"])?;
            let tag = util::required_option(&mut options, "tag")?;
            util::ensure_empty_options(options)?;
            release::validate_release(&tag)
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
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid --settle-milliseconds {value:?}: {error}"))
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
            util::ensure_empty_options(options)?;
            zed_gui::run(
                &zed,
                &profile,
                Duration::from_secs(timeout),
                Duration::from_millis(settle),
                allow_input,
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
        other => Err(format!(
            "unknown xtask command {other:?}; run `cargo xtask help`"
        )),
    }
}

fn print_help() {
    println!(
        "Repository automation

Usage:
  cargo xtask check-dependencies [--target <target>]
  cargo xtask collect-licenses --target <target> [--output <dir>]
  cargo xtask package-release --target <target> [--output <dir>]
  cargo xtask validate-release --tag <vX.Y.Z>
  cargo xtask verify-release-assets [--input <dir>]
  cargo xtask check-no-python
  cargo xtask test-zed [--zed <binary>] [--profile <dir>] [--timeout-seconds <5-180>]
  cargo xtask test-zed-gui [--zed <binary>] [--profile <dir>] [--timeout-seconds <5-180>] [--settle-milliseconds <100-5000>] --allow-input-injection true"
    );
}
