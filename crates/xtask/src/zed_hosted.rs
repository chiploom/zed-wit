use crate::{util, zed_smoke};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const SERVER_PREFIX: &str = "wit-language-server";

struct ReleaseIdentity {
    version: String,
    tag: String,
    sha: String,
}

struct ObservedServer {
    pid: u32,
    path: PathBuf,
}

pub fn run(zed: &str, profile: &Path, timeout: Duration) -> Result<(), String> {
    if !cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )) {
        return Err("test-zed-hosted supports Zed desktop hosts: macOS, Linux, and Windows".into());
    }

    let root = util::repo_root();
    zed_smoke::validate_disposable_profile(&root, profile)?;
    let release = release_identity(&root)?;
    let zed_path = zed_smoke::resolve_executable(zed)?;
    let zed_version = command_output(&zed_path, &["--version"], &root)?;

    log(format!("repository: {}", root.display()));
    log(format!("release: {} ({})", release.tag, release.sha));
    log(format!("Zed: {zed_version}"));
    log(format!("isolated profile: {}", profile.display()));

    run_status(
        "cargo",
        &["build", "--target", "wasm32-wasip2", "--locked"],
        &root,
    )?;

    let staged = zed_smoke::stage_hosted(&root, profile)?;
    let override_path = staged.workspace_dir.join(".zed/settings.json");
    if override_path.exists() {
        return Err(format!(
            "hosted qualification must not stage a local LSP override: {}",
            override_path.display()
        ));
    }

    let first = run_session(&zed_path, profile, &staged, "first-install", timeout)?;
    ensure_profile_owned(profile, &first.path)?;
    let checksum = checksum_path(&first.path)?;
    let initial_digest = util::sha256_file(&first.path)?;
    verify_checksum_sidecar(&checksum, &first.path, &initial_digest)?;
    verify_server_identity(&first.path, &release, &root)?;

    let binary_modified = modified(&first.path)?;
    let checksum_modified = modified(&checksum)?;

    // Give coarse-timestamp filesystems enough separation that a hidden
    // re-download during the cache test cannot look like an unchanged file.
    thread::sleep(Duration::from_millis(1100));

    let cached = run_session(&zed_path, profile, &staged, "cached-install", timeout)?;
    require_same_path(&first.path, &cached.path, "cached install")?;
    if util::sha256_file(&cached.path)? != initial_digest {
        return Err("cached hosted binary digest changed between clean launches".into());
    }
    verify_checksum_sidecar(&checksum, &cached.path, &initial_digest)?;
    if modified(&cached.path)? != binary_modified || modified(&checksum)? != checksum_modified {
        return Err(
            "cached hosted install rewrote the verified binary or checksum instead of reusing it"
                .into(),
        );
    }

    fs::write(&first.path, b"corrupted hosted cache\n")
        .map_err(|error| format!("corrupt {}: {error}", first.path.display()))?;
    if util::sha256_file(&first.path)? == initial_digest {
        return Err("failed to corrupt hosted cache fixture".into());
    }

    let repaired = run_session(
        &zed_path,
        profile,
        &staged,
        "corrupt-cache-recovery",
        timeout,
    )?;
    require_same_path(&first.path, &repaired.path, "corrupt cache recovery")?;
    if util::sha256_file(&repaired.path)? != initial_digest {
        return Err("corrupt hosted binary was not restored to the published digest".into());
    }
    verify_checksum_sidecar(&checksum, &repaired.path, &initial_digest)?;
    verify_server_identity(&repaired.path, &release, &root)?;

    fs::remove_file(&checksum)
        .map_err(|error| format!("remove {}: {error}", checksum.display()))?;
    if checksum.exists() {
        return Err(format!(
            "failed to remove hosted checksum fixture: {}",
            checksum.display()
        ));
    }

    let missing_checksum = run_session(
        &zed_path,
        profile,
        &staged,
        "missing-checksum-recovery",
        timeout,
    )?;
    require_same_path(
        &first.path,
        &missing_checksum.path,
        "missing checksum recovery",
    )?;
    if util::sha256_file(&missing_checksum.path)? != initial_digest {
        return Err("missing-checksum recovery changed the published binary digest".into());
    }
    verify_checksum_sidecar(&checksum, &missing_checksum.path, &initial_digest)?;
    verify_server_identity(&missing_checksum.path, &release, &root)?;

    let report = json!({
        "result": "passed",
        "release_tag": release.tag,
        "release_sha": release.sha,
        "release_version": release.version,
        "zed_version": zed_version,
        "os": env::consts::OS,
        "arch": env::consts::ARCH,
        "profile": profile,
        "downloaded_server": first.path,
        "sha256": initial_digest,
        "scenarios": [
            {
                "scenario": "first_hosted_install",
                "result": "passed",
                "evidence": "fresh isolated Zed profile had no local LSP override or WIT server on PATH; the extension downloaded, verified, and launched the published native server",
            },
            {
                "scenario": "cached_install",
                "result": "passed",
                "evidence": "second launch reused the same verified server and checksum without rewriting either cache file",
            },
            {
                "scenario": "corrupt_cache_recovery",
                "result": "passed",
                "evidence": "a deliberately corrupted cached executable was rejected and replaced with bytes matching the original published SHA-256 digest",
            },
            {
                "scenario": "missing_checksum_recovery",
                "result": "passed",
                "evidence": "a deliberately removed checksum sidecar caused a clean recovery; the checksum was restored and the launched server matched the published digest and build identity",
            }
        ],
    });
    let report_path = profile.join("zed-hosted-report.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("encode hosted qualification report: {error}"))?,
    )
    .map_err(|error| format!("write {}: {error}", report_path.display()))?;

    eprintln!("[test-zed-hosted] report: {}", report_path.display());
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("encode hosted qualification report: {error}"))?
    );
    Ok(())
}

fn run_session(
    zed: &Path,
    profile: &Path,
    staged: &zed_smoke::Staged,
    label: &str,
    timeout: Duration,
) -> Result<ObservedServer, String> {
    let stdout_log = profile.join(format!("zed-hosted-{label}.stdout.log"));
    let stderr_log = profile.join(format!("zed-hosted-{label}.stderr.log"));
    let before = hosted_server_processes()?
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();

    log(format!("launching {label}"));
    let mut child = zed_smoke::launch(
        zed,
        profile,
        &staged.workspace_dir,
        &staged.wit_files,
        &stdout_log,
        &stderr_log,
    )?;

    let observed = match wait_for_hosted_server(&mut child, &before, profile, timeout) {
        Ok(observed) => observed,
        Err(error) => {
            let _ = zed_smoke::stop_zed(&mut child);
            return Err(error);
        }
    };

    zed_smoke::stop_zed(&mut child)?;
    zed_smoke::ensure_server_stopped(observed.pid, &observed.path, Duration::from_secs(3))?;
    zed_smoke::scan_logs(profile, &stdout_log, &stderr_log)?;
    Ok(observed)
}

fn wait_for_hosted_server(
    child: &mut Child,
    before: &BTreeSet<u32>,
    profile: &Path,
    timeout: Duration,
) -> Result<ObservedServer, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let current = hosted_server_processes()?;
        let candidates = current
            .iter()
            .filter(|(pid, _)| !before.contains(pid))
            .map(|(pid, path)| (*pid, path.clone()))
            .collect::<Vec<_>>();

        if candidates.len() > 1 {
            return Err(format!(
                "multiple new WIT language-server processes appeared during hosted qualification: {candidates:?}"
            ));
        }
        if let Some((pid, path)) = candidates.into_iter().next() {
            thread::sleep(Duration::from_millis(750));
            if hosted_server_processes()?.get(&pid) == Some(&path) {
                log(format!(
                    "hosted language server is running (PID {pid}): {}",
                    path.display()
                ));
                return Ok(ObservedServer { pid, path });
            }
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("poll isolated Zed process: {error}"))?
            && !status.success()
        {
            return Err(format!(
                "Zed exited with {status} before starting the hosted WIT server; inspect {}",
                profile.display()
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(format!(
        "timed out after {}s waiting for Zed to start a downloaded WIT server; inspect {}",
        timeout.as_secs(),
        profile.display()
    ))
}

fn hosted_server_processes() -> Result<BTreeMap<u32, PathBuf>, String> {
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        return Err("process inspection is unsupported on this host".into());
    }

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );

    Ok(system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let path = process.exe()?.to_path_buf();
            is_language_server_executable(&path).then_some((pid.as_u32(), path))
        })
        .collect())
}

fn is_language_server_executable(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name == SERVER_PREFIX || name.starts_with(&format!("{SERVER_PREFIX}-"))
}

fn release_identity(root: &Path) -> Result<ReleaseIdentity, String> {
    let manifest_path = root.join("crates/wit-language-server/Cargo.toml");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&source)
        .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
    let version = manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or("language-server manifest omitted package.version")?
        .to_owned();
    let tag = format!("v{version}");
    let output = util::command_output(
        "git",
        ["ls-remote", "--refs", "origin", &format!("refs/tags/{tag}")],
        root,
    )?;
    let sha = parse_remote_tag(&output, &tag)?;
    Ok(ReleaseIdentity { version, tag, sha })
}

fn parse_remote_tag(output: &str, tag: &str) -> Result<String, String> {
    let expected_ref = format!("refs/tags/{tag}");
    let mut matches = output.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let sha = fields.next()?;
        let reference = fields.next()?;
        (reference == expected_ref).then_some(sha)
    });
    let sha = matches
        .next()
        .ok_or_else(|| format!("published release tag {tag} was not found on origin"))?;
    if matches.next().is_some() {
        return Err(format!(
            "origin returned multiple refs for release tag {tag}"
        ));
    }
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "origin returned an invalid commit for release tag {tag}: {sha:?}"
        ));
    }
    Ok(sha.to_owned())
}

fn verify_server_identity(
    server: &Path,
    release: &ReleaseIdentity,
    root: &Path,
) -> Result<(), String> {
    let actual = command_output(server, &["--version"], root)?;
    let expected = format!("{}+git.{}", release.version, release.sha);
    if !actual.contains(&expected) {
        return Err(format!(
            "downloaded server identity mismatch: expected {expected:?}, got {actual:?}"
        ));
    }
    Ok(())
}

fn verify_checksum_sidecar(checksum: &Path, server: &Path, digest: &str) -> Result<(), String> {
    let name = server
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("non-UTF-8 server filename: {}", server.display()))?;
    let expected = format!("{digest}  {name}\n");
    let actual = fs::read_to_string(checksum)
        .map_err(|error| format!("read {}: {error}", checksum.display()))?;
    if actual != expected {
        return Err(format!(
            "hosted checksum sidecar does not match downloaded server: {}",
            checksum.display()
        ));
    }
    Ok(())
}

fn checksum_path(server: &Path) -> Result<PathBuf, String> {
    let name = server
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("non-UTF-8 server filename: {}", server.display()))?;
    Ok(server.with_file_name(format!("{name}.sha256")))
}

fn ensure_profile_owned(profile: &Path, server: &Path) -> Result<(), String> {
    let profile = fs::canonicalize(profile)
        .map_err(|error| format!("canonicalize {}: {error}", profile.display()))?;
    let server = fs::canonicalize(server)
        .map_err(|error| format!("canonicalize {}: {error}", server.display()))?;
    if !server.starts_with(&profile) {
        return Err(format!(
            "hosted qualification launched a WIT server outside the isolated profile: {}",
            server.display()
        ));
    }
    Ok(())
}

fn require_same_path(expected: &Path, actual: &Path, scenario: &str) -> Result<(), String> {
    if expected != actual {
        return Err(format!(
            "{scenario} launched a different server path: expected {}, got {}",
            expected.display(),
            actual.display()
        ));
    }
    Ok(())
}

fn modified(path: &Path) -> Result<SystemTime, String> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| format!("read modification time for {}: {error}", path.display()))
}

fn command_output(program: &Path, args: &[&str], root: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("run {}: {error}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            program.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("decode {} output: {error}", program.display()))
}

fn run_status(program: &str, args: &[&str], root: &Path) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {args:?} exited with {status}"))
    }
}

fn log(message: impl std::fmt::Display) {
    eprintln!("[test-zed-hosted] {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_release_server_names() {
        assert!(is_language_server_executable(Path::new(
            "/tmp/wit-language-server-aarch64-apple-darwin"
        )));
        assert!(is_language_server_executable(Path::new(
            "C:/tmp/wit-language-server-x86_64-pc-windows-msvc.exe"
        )));
        assert!(is_language_server_executable(Path::new(
            "/tmp/wit-language-server"
        )));
        assert!(!is_language_server_executable(Path::new(
            "/tmp/other-language-server"
        )));
    }

    #[test]
    fn parses_exact_remote_release_tag() {
        let output = "0123456789abcdef0123456789abcdef01234567\trefs/tags/v0.1.0\n";
        assert_eq!(
            parse_remote_tag(output, "v0.1.0").unwrap(),
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert!(parse_remote_tag(output, "v0.2.0").is_err());
    }
}
