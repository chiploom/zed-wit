//! Local developer automation. Publication stays in the protected CD workflow.
use crate::{dependency_policy, licenses, release, repository_policy, util};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const COMMANDS: &[&str] = &[
    "release-build", "release", "verify", "test", "build", "check", "dev",
    "test-all", "release-check", "doctor", "clean", "test-lsp",
    "test-extension", "install-dev", "bench", "coverage",
    "changelog-check", "update-grammar",
];

pub fn dispatch(command: &str, args: &[String]) -> Result<(), String> {
    if !COMMANDS.contains(&command) {
        return Err(format!(
            "unknown xtask command {command:?}; run cargo xtask help"
        ));
    }
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        show_help(command);
        return Ok(());
    }
    match command {
        "release-build" => release_build(args),
        "release" => prepare_release(args),
        "verify" => verify(args),
        "test" => test(args),
        "build" => build(args),
        "check" => check(args),
        "dev" => dev(args),
        "test-all" => test_all(args),
        "release-check" => release_check(args),
        "doctor" => doctor(args),
        "clean" => clean(args),
        "test-lsp" => test_lsp(args),
        "test-extension" => test_extension(args),
        "install-dev" => install_dev(args),
        "bench" => bench(args),
        "coverage" => coverage(args),
        "changelog-check" => changelog_check(args),
        "update-grammar" => update_grammar(args),
        _ => unreachable!("all commands are listed above"),
    }
}

pub fn help() {
    println!("Developer commands (run cargo xtask <command> --help for options):");
    for command in COMMANDS {
        println!("  cargo xtask {command}");
    }
    println!("Release creation and publication remain protected CD-only actions.");
}

fn show_help(command: &str) {
    let options = match command {
        "release-build" => "[--target <release-target>] [--output <binary-path>]",
        "release" => "--scope <lsp|extension> --tag <tag> [--target <release-target>] [--output <dir>]",
        "test" => "[--package <name>] [--filter <substring>] [--runner <auto|cargo|nextest>]",
        "build" => "[--kind <server|extension>] [--release <true|false>] [--target <triple>]",
        "test-all" => "[--with-zed <true|false>]",
        "release-check" => "--scope <lsp|extension> --tag <tag>",
        "clean" => "--scope <dist|profiles|coverage|build|all> [--execute <true|false>]",
        "install-dev" => "--destination <binary-path>",
        "bench" => "[--iterations <positive-integer>]",
        "coverage" => "[--output <path-under-target>]",
        "changelog-check" => "[--scope <lsp|extension> --tag <tag>]",
        "update-grammar" => "[--candidate <40-character-git-sha>]",
        _ => "(no options)",
    };
    println!("Usage: cargo xtask {command} {options}");
    if matches!(command, "release" | "release-check" | "release-build") {
        println!("Local preparation only. No GitHub release or protected tag is created.");
    }
    if command == "update-grammar" {
        println!("Read-only audit: never changes pinned grammar revisions.");
    }
}

fn no_args(args: &[String], name: &str) -> Result<(), String> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!("{name} does not accept arguments; see --help"))
    }
}

fn opts(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>, String> {
    util::parse_options(args, allowed)
}

fn bool_option(options: &mut BTreeMap<String, String>, key: &str, default: bool) -> Result<bool, String> {
    match options.remove(key).as_deref() {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(value) => Err(format!("--{key} must be true or false, got {value:?}")),
    }
}

fn finish(options: BTreeMap<String, String>) -> Result<(), String> {
    util::ensure_empty_options(options)
}

fn run(program: &str, args: &[String]) -> Result<(), String> {
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

fn cargo(args: &[&str]) -> Result<(), String> {
    run("cargo", &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
}

fn tool_available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(util::repo_root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn host_target() -> Result<String, String> {
    let verbose = util::command_output("rustc", ["-vV"], &util::repo_root())?;
    let host = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| "rustc -vV omitted the host target".to_owned())?;
    Ok(host.to_owned())
}

fn native_binary(target: Option<&str>, release_profile: bool) -> PathBuf {
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

fn ensure_regular_file(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if !meta.file_type().is_file() || meta.len() == 0 {
        return Err(format!("expected a nonempty regular file: {}", path.display()));
    }
    Ok(())
}

fn copy_new_file(source: &Path, destination: &Path) -> Result<(), String> {
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

fn release_build(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["target", "output"])?;
    let target = match options.remove("target") {
        Some(target) => target,
        None => host_target()?,
    };
    dependency_policy::ensure_target(&target)?;
    let output = options.remove("output").map(|value| util::root_relative(value.into()));
    finish(options)?;
    cargo(&["build", "-p", "wit-language-server", "--release", "--locked", "--target", &target])?;
    if let Some(destination) = output {
        copy_new_file(&native_binary(Some(&target), true), &destination)?;
    }
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    no_args(args, "check")?;
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&["clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"])?;
    cargo(&["check", "--workspace", "--locked"])
}

fn test(args: &[String]) -> Result<(), String> {
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
    let nextest = filter.is_none()
        && runner != "cargo"
        && tool_available("cargo", &["nextest", "--version"]);
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

fn build(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["kind", "release", "target"])?;
    let kind = options.remove("kind").unwrap_or_else(|| "server".into());
    let optimized = bool_option(&mut options, "release", false)?;
    let target = options.remove("target");
    finish(options)?;
    let mut command = vec!["build".to_owned(), "--locked".to_owned()];
    match kind.as_str() {
        "server" => {
            command.extend(["-p".to_owned(), "wit-language-server".into()]);
            if let Some(target) = target {
                command.extend(["--target".into(), target]);
            }
        }
        "extension" => {
            if target.as_deref().is_some_and(|v| v != "wasm32-wasip2") {
                return Err("Zed extension must target wasm32-wasip2".into());
            }
            command.extend(["-p".into(), "zed-wit".into(), "--target".into(), "wasm32-wasip2".into()]);
        }
        _ => return Err(format!("unknown build kind {kind:?}")),
    }
    if optimized {
        command.push("--release".into());
    }
    run("cargo", &command)
}

fn dev(args: &[String]) -> Result<(), String> {
    no_args(args, "dev")?;
    let template = util::read_nonempty(&util::repo_root().join(".zed/settings.example.json"))?;
    if !template.contains("target/release/wit-language-server") {
        return Err("example Zed settings no longer point to the local release language server".into());
    }
    build(&["--kind".into(), "server".into(), "--release".into(), "true".into()])?;
    println!("Use .zed/settings.example.json as a template for untracked .zed/settings.json.");
    println!("No user or project settings were modified.");
    Ok(())
}

fn test_lsp(args: &[String]) -> Result<(), String> {
    no_args(args, "test-lsp")?;
    for package in ["wit-syntax", "wit-analysis", "wit-language-server"] {
        test(&["--package".into(), package.into()])?;
    }
    Ok(())
}

fn test_extension(args: &[String]) -> Result<(), String> {
    no_args(args, "test-extension")?;
    test(&["--package".into(), "zed-wit".into()])?;
    test(&["--package".into(), "wit-syntax".into()])?;
    build(&["--kind".into(), "extension".into()])
}

fn test_all(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["with-zed"])?;
    let with_zed = bool_option(&mut options, "with-zed", false)?;
    finish(options)?;
    test(&[])?;
    cargo(&["test", "--doc", "--workspace", "--exclude", "xtask", "--locked"])?;
    build(&["--kind".into(), "extension".into()])?;
    if with_zed {
        crate::zed_smoke::run(
            "zed",
            &util::repo_root().join("target/zed-smoke/profile"),
            std::time::Duration::from_secs(90),
        )?;
    } else {
        eprintln!("SKIPPED: real-Zed smoke/hosted/GUI tests (opt in with --with-zed true; hosted/GUI retain dedicated commands)");
    }
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    no_args(args, "verify")?;
    repository_policy::check_no_python()?;
    dependency_policy::run(None)?;
    check(&[])?;
    test_all(&[])
}

fn changelog_check_inner(scope: Option<&str>, tag: Option<&str>) -> Result<(), String> {
    let root = util::repo_root();
    let changelog = util::read_nonempty(&root.join("CHANGELOG.md"))?;
    if !changelog.contains("# Changelog") || !changelog.contains("## Unreleased") {
        return Err("CHANGELOG.md needs a changelog heading and Unreleased section".into());
    }
    match (scope, tag) {
        (Some(scope), Some(tag)) => {
            let maybe_version = match scope {
                "lsp" => tag.strip_prefix('v'),
                "extension" => tag.strip_prefix("v-extension-"),
                _ => return Err(format!("unsupported release scope {scope:?}")),
            };
            let version = maybe_version.ok_or_else(|| "tag does not match release scope".to_owned())?;
            if version.split('.').count() != 3
                || !version.split('.').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')))
            {
                return Err(format!("invalid stable SemVer release tag: {tag}"));
            }
            let path = root.join(format!("docs/releases/{scope}/v{version}.md"));
            let notes = util::read_nonempty(&path)?;
            if notes.trim().len() < 20 {
                return Err(format!("release notes too short: {}", path.display()));
            }
            println!("validated release notes: {}", path.display());
        }
        (None, None) => {}
        _ => return Err("--scope and --tag must be supplied together".into()),
    }
    println!("changelog validation passed");
    Ok(())
}

fn changelog_check(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag"])?;
    let scope = options.remove("scope");
    let tag = options.remove("tag");
    finish(options)?;
    changelog_check_inner(scope.as_deref(), tag.as_deref())
}

fn release_check_inner(scope: &str, tag: &str) -> Result<(), String> {
    changelog_check_inner(Some(scope), Some(tag))?;
    release::validate_release(tag, scope)
}

fn release_check(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let tag = util::required_option(&mut options, "tag")?;
    finish(options)?;
    release_check_inner(&scope, &tag)
}

fn prepare_release(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "tag", "target", "output"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let tag = util::required_option(&mut options, "tag")?;
    let target = options.remove("target");
    let specified_output = options.remove("output");
    if scope == "extension" && specified_output.is_some() {
        return Err("--output is only supported for LSP release preparation".into());
    }
    let output = util::root_relative(
        specified_output.unwrap_or_else(|| "dist".into()).into()
    );
    finish(options)?;
    if scope == "extension" && target.is_some() {
        return Err("extension release preparation does not accept --target".into());
    }
    // Validate before building so invalid candidate releases perform no build work.
    release_check_inner(&scope, &tag)?;
    match scope.as_str() {
        "lsp" => {
            let target = target.ok_or_else(|| "LSP preparation requires --target (native release triple)".to_owned())?;
            dependency_policy::ensure_target(&target)?;
            release_build(&["--target".into(), target.clone()])?;
            release::package_release(&target, &output)?;
            licenses::run(&target, &output)?;
            release::verify_release_target(&target, &output)?;
            println!("Local release artifacts prepared. Complete five-target CD validation and protected publication separately.");
        }
        "extension" => {
            build(&["--kind".into(), "extension".into(), "--release".into(), "true".into()])?;
            println!("Extension Wasm candidate built locally. Published LSP dependency and protected CD publication must be verified separately.");
        }
        _ => return Err(format!("unsupported release scope {scope:?}")),
    }
    Ok(())
}

fn doctor(args: &[String]) -> Result<(), String> {
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
            errors.push(format!("missing {name}; install the Rust toolchain or Git as appropriate"));
        }
    }
    if let Ok(installed) = util::command_output("rustup", ["target", "list", "--installed"], &root) {
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
        errors.push("missing a native C compiler; install Xcode CLT, GCC/Clang, or MSVC Build Tools".into());
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
            println!("OPTIONAL UNAVAILABLE: {name}; its specific commands may require installation");
        }
    }
    if errors.is_empty() {
        println!("Required environment checks passed; this does not qualify real-Zed/platform release behavior.");
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn safe_generated_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    const ALLOWED: &[&str] = &[
        "dist", "target/coverage", "target/zed-gui", "target/zed-hosted", "target/zed-smoke"
    ];
    if !ALLOWED.contains(&relative) {
        return Err(format!("cleanup path is not allowlisted: {relative}"));
    }
    if relative.starts_with("target/") {
        let target = root.join("target");
        if let Ok(meta) = fs::symlink_metadata(&target) {
            if meta.file_type().is_symlink() {
                return Err("refusing cleanup through a symlinked target directory".into());
            }
        }
    }
    let path = root.join(relative);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(format!("refusing cleanup of non-directory or symlink: {}", path.display()));
        }
    }
    Ok(path)
}

fn clean(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["scope", "execute"])?;
    let scope = util::required_option(&mut options, "scope")?;
    let execute = bool_option(&mut options, "execute", false)?;
    finish(options)?;
    let mut paths = match scope.as_str() {
        "dist" => vec!["dist"],
        "profiles" => vec!["target/zed-smoke", "target/zed-hosted", "target/zed-gui"],
        "coverage" => vec!["target/coverage"],
        "build" => vec![],
        "all" => vec!["dist", "target/zed-smoke", "target/zed-hosted", "target/zed-gui", "target/coverage"],
        _ => return Err(format!("invalid cleanup scope {scope:?}")),
    };
    let root = util::repo_root();
    let resolved = paths.drain(..)
        .map(|p| safe_generated_path(&root, p))
        .collect::<Result<Vec<_>, _>>()?;
    if matches!(scope.as_str(), "build" | "all") && root.join("target").is_symlink() {
        return Err("refusing cargo clean through a symlinked target directory".into());
    }
    if execute && cfg!(windows) && matches!(scope.as_str(), "build" | "all") {
        return Err("cannot remove a running xtask.exe on Windows; run cargo clean --locked directly".into());
    }
    for path in resolved {
        if execute && path.exists() {
            fs::remove_dir_all(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            println!("removed {}", path.display());
        } else {
            println!("{} {}", if execute { "not present:" } else { "dry-run:" }, path.display());
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

fn install_dev(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["destination"])?;
    let destination = util::root_relative(
        util::required_option(&mut options, "destination")?.into()
    );
    finish(options)?;
    let source = native_binary(None, true);
    copy_new_file(&source, &destination)
}

fn bench(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["iterations"])?;
    let iterations = options.remove("iterations").unwrap_or_else(|| "2000".into());
    finish(options)?;
    let count = iterations.parse::<u32>()
        .map_err(|e| format!("invalid --iterations {iterations:?}: {e}"))?;
    if !(10..=1_000_000).contains(&count) {
        return Err("--iterations must be between 10 and 1000000".into());
    }
    cargo(&["bench", "-p", "wit-syntax", "--bench", "parse", "--locked", "--", "--iterations", &iterations])
}

fn coverage(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["output"])?;
    let value = options.remove("output").unwrap_or_else(|| "target/coverage/lcov.info".into());
    finish(options)?;
    let rel = Path::new(&value);
    let suffix = rel.strip_prefix("target/coverage")
        .map_err(|_| "coverage output must be beneath target/coverage/".to_owned())?;
    if suffix.components().count() != 1
        || !matches!(suffix.components().next(), Some(std::path::Component::Normal(_)))
    {
        return Err("coverage output must be one filename beneath target/coverage/".into());
    }
    if !tool_available("cargo", &["llvm-cov", "--version"]) {
        return Err("cargo-llvm-cov is required; install with cargo install cargo-llvm-cov --locked".into());
    }
    let dir = util::repo_root().join("target/coverage");
    if util::repo_root().join("target").is_symlink() || dir.is_symlink()
        || util::repo_root().join(&value).is_symlink()
    {
        return Err("refusing coverage output through a symlinked build directory or file".into());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    cargo(&["llvm-cov", "--workspace", "--exclude", "xtask", "--locked", "--lcov", "--output-path", &value])
}

fn update_grammar(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["candidate"])?;
    let candidate = options.remove("candidate");
    finish(options)?;
    let root = util::repo_root();
    let extension: toml::Value = toml::from_str(&util::read_nonempty(&root.join("extension.toml"))?)
        .map_err(|e| format!("parse extension.toml: {e}"))?;
    let syntax: toml::Value = toml::from_str(&util::read_nonempty(&root.join("crates/wit-syntax/Cargo.toml"))?)
        .map_err(|e| format!("parse wit-syntax Cargo.toml: {e}"))?;
    let ext_rev = extension.get("grammars").and_then(|v| v.get("wit"))
        .and_then(|v| v.get("rev")).and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml has no grammars.wit.rev".to_owned())?;
    let cargo_rev = syntax.get("dependencies").and_then(|v| v.get("tree-sitter-wit"))
        .and_then(|v| v.get("rev")).and_then(toml::Value::as_str)
        .ok_or_else(|| "wit-syntax manifest has no tree-sitter-wit.rev".to_owned())?;
    if ext_rev != cargo_rev {
        return Err(format!("grammar pin mismatch: extension={ext_rev} syntax={cargo_rev}"));
    }
    let notes = util::read_nonempty(&root.join("docs/upstream-compatibility.md"))?;
    if !notes.contains(ext_rev) {
        return Err("upstream-compatibility.md does not document the pinned grammar revision".into());
    }
    println!("Current pinned grammar: {ext_rev} (extension and syntax manifests agree)");
    if let Some(candidate) = candidate {
        if candidate.len() != 40 || !candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("--candidate requires a 40-character hexadecimal Git commit".into());
        }
        println!("Candidate: {candidate}");
        println!("Review upstream grammar delta, query captures and fixtures, and update both pins, Cargo.lock and dated compatibility evidence together.");
    }
    println!("Read-only audit complete. No pin or source changed; see issue #17 for requalification.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_approved_commands_are_registered() {
        assert_eq!(COMMANDS.len(), 18);
        for cmd in COMMANDS {
            assert!(dispatch(cmd, &["--help".into()]).is_ok(), "{cmd}");
        }
    }
    #[test]
    fn unknown_command_is_rejected() {
        assert!(dispatch("publish", &[]).is_err());
        assert!(dispatch("watch", &[]).is_err());
    }
    #[test]
    fn boolean_options_are_strict() {
        let mut options = BTreeMap::from([("execute".into(), "maybe".into())]);
        assert!(bool_option(&mut options, "execute", false).is_err());
        let mut options = BTreeMap::from([("execute".into(), "true".into())]);
        assert!(bool_option(&mut options, "execute", false).unwrap());
    }
    #[test]
    fn generated_cleanup_paths_are_allowlisted() {
        let root = util::repo_root();
        assert!(safe_generated_path(&root, "target/coverage").is_ok());
        assert!(safe_generated_path(&root, ".git").is_err());
        assert!(safe_generated_path(&root, "target/../src").is_err());
    }
    #[test]
    fn release_commands_validate_options_before_spawning() {
        assert!(release_check(&["--scope".into(), "lsp".into()]).is_err());
        assert!(release_build(&["--target".into(), "unknown".into()]).is_err());
        assert!(prepare_release(&["--scope".into(), "extension".into()]).is_err());
    }
    #[test]
    fn coverage_rejects_traversal_before_using_optional_tool() {
        assert!(coverage(&["--output".into(), "target/coverage/../Cargo.toml".into()]).is_err());
        assert!(coverage(&["--output".into(), "/tmp/test.lcov".into()]).is_err());
    }
    #[test]
    fn grammar_candidate_input_is_validated_without_network() {
        assert!(update_grammar(&["--candidate".into(), "bad".into()]).is_err());
    }
}
