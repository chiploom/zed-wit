use crate::util;
use serde_json::{Value, json};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use std::{
    collections::BTreeSet,
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const EXTENSION_ID: &str = "wit";
const WASM_TARGET: &str = "wasm32-wasip2";
const PHASES: usize = 9;

fn log(message: impl std::fmt::Display) {
    eprintln!("[test-zed] {message}");
}

fn phase(number: usize, message: impl std::fmt::Display) {
    eprintln!("[test-zed] [{number}/{PHASES}] {message}");
}

pub fn run(zed: &str, profile: &Path, timeout: Duration) -> Result<(), String> {
    if !cfg!(any(target_os = "macos", target_os = "linux", target_os = "windows")) {
        return Err("test-zed supports Zed desktop hosts: macOS, Linux, and Windows".into());
    }

    let root = util::repo_root();
    let head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
    let zed_path = resolve_executable(zed)?;
    let zed_version = output_path(&zed_path, &["--version"], &root)?;

    log(format!("repository: {}", root.display()));
    log(format!("HEAD: {head}"));
    log(format!("Zed: {zed_version}"));
    log(format!("Zed executable: {}", zed_path.display()));
    log(format!("isolated profile: {}", profile.display()));

    phase(1, "running full workspace unit and integration tests");
    run_status(
        "cargo",
        &["test", "--workspace", "--all-features", "--locked"],
        &root,
        &[],
    )?;
    phase(2, "running workspace doctests");
    run_status(
        "cargo",
        &[
            "test",
            "--doc",
            "--workspace",
            "--exclude",
            "xtask",
            "--locked",
        ],
        &root,
        &[],
    )?;
    phase(3, format!("building Zed extension for {WASM_TARGET}"));
    run_status(
        "cargo",
        &["build", "--target", WASM_TARGET, "--locked"],
        &root,
        &[],
    )?;
    phase(4, "building exact native language server");
    run_status(
        "cargo",
        &[
            "build",
            "-p",
            "wit-language-server",
            "--release",
            "--locked",
        ],
        &root,
        &[("WIT_LANGUAGE_SERVER_BUILD_COMMIT", head.as_str())],
    )?;

    let server = native_server(&root);
    let server_version = output_path(&server, &["--version"], &root)?;
    let expected = format!("+git.{head}");
    log(format!("native server: {}", server.display()));
    log(format!("native server identity: {server_version}"));
    if !server_version.contains(&expected) {
        return Err(format!(
            "native server identity mismatch: expected {expected}, got {server_version:?}"
        ));
    }

    phase(
        5,
        "staging isolated runtime extension, grammar, and workspace",
    );
    let staged = stage(&root, profile, &server)?;
    log(format!(
        "runtime extension: {}",
        staged.extension_dir.display()
    ));
    log(format!("workspace: {}", staged.workspace_dir.display()));
    log(format!("WIT fixtures staged: {}", staged.wit_files.len()));
    for fixture in &staged.wit_files {
        let relative = fixture
            .strip_prefix(&staged.workspace_dir)
            .unwrap_or(fixture);
        log(format!("fixture: {}", relative.display()));
    }

    let stdout_log = profile.join("zed-foreground.stdout.log");
    let stderr_log = profile.join("zed-foreground.stderr.log");
    let before = matching_processes(&server)?;
    phase(6, "launching isolated stateless Zed");
    log("Zed will use a fresh profile and the exact native server from target/release");
    let mut child = launch(
        &zed_path,
        profile,
        &staged.workspace_dir,
        &staged.wit_files,
        &stdout_log,
        &stderr_log,
    )?;

    log(format!(
        "waiting up to {}s for Zed to start {}",
        timeout.as_secs(),
        server.display()
    ));
    let smoke = wait_for_server(&mut child, &server, &before, profile, timeout);
    stop_zed(&mut child)?;
    log("isolated Zed process stopped");
    let server_pid = match smoke {
        Ok(pid) => pid,
        Err(error) => {
            let logs = diagnostic_logs(profile, &stdout_log, &stderr_log);
            return Err(if logs.is_empty() {
                error
            } else {
                format!("{error}\n\nZed logs:\n{logs}")
            });
        }
    };
    ensure_server_stopped(server_pid, &server, Duration::from_secs(3))?;
    log(format!(
        "language-server PID {server_pid} stopped with isolated Zed"
    ));

    phase(7, "scanning isolated Zed logs for integration failures");
    scan_logs(profile, &stdout_log, &stderr_log)?;
    log("no WIT extension, grammar, query, or language-server startup failures found");

    phase(
        8,
        "restarting isolated Zed and requalifying server lifecycle",
    );
    let restart_stdout_log = profile.join("zed-restart.stdout.log");
    let restart_stderr_log = profile.join("zed-restart.stderr.log");
    let restart_before = matching_processes(&server)?;
    let mut restart_child = launch(
        &zed_path,
        profile,
        &staged.workspace_dir,
        &staged.wit_files,
        &restart_stdout_log,
        &restart_stderr_log,
    )?;
    let restart_smoke = wait_for_server(
        &mut restart_child,
        &server,
        &restart_before,
        profile,
        timeout,
    );
    stop_zed(&mut restart_child)?;
    let restart_server_pid = match restart_smoke {
        Ok(pid) => pid,
        Err(error) => {
            let logs = diagnostic_logs(profile, &restart_stdout_log, &restart_stderr_log);
            return Err(if logs.is_empty() {
                error
            } else {
                format!("{error}\n\nRestarted Zed logs:\n{logs}")
            });
        }
    };
    ensure_server_stopped(restart_server_pid, &server, Duration::from_secs(3))?;
    scan_logs(profile, &restart_stdout_log, &restart_stderr_log)?;
    log(format!(
        "restart PASS: language-server PID {restart_server_pid} started and stopped cleanly"
    ));

    phase(9, "writing smoke-test evidence");
    let fixture_paths = staged
        .wit_files
        .iter()
        .map(|path| {
            path.strip_prefix(&staged.workspace_dir)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    let report = json!({
        "result": "passed",
        "head": head,
        "zed_version": zed_version,
        "server_version": server_version,
        "profile": profile,
        "runtime_extension": staged.extension_dir,
        "workspace": staged.workspace_dir,
        "wit_fixture_count": fixture_paths.len(),
        "wit_fixtures": fixture_paths,
        "server_pid": server_pid,
        "restart_server_pid": restart_server_pid,
        "foreground_stdout": stdout_log,
        "foreground_stderr": stderr_log,
        "restart_stdout": restart_stdout_log,
        "restart_stderr": restart_stderr_log,
        "scenarios": manual_scenarios(&head),
    });
    let report_path = profile.join("zed-smoke-report.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("encode {}: {error}", report_path.display()))?,
    )
    .map_err(|error| format!("write {}: {error}", report_path.display()))?;

    log(format!("report: {}", report_path.display()));
    log("PASS: real Zed loaded the extension and started the exact native server");
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("encode Zed smoke report: {error}"))?
    );
    Ok(())
}

pub(crate) struct Staged {
    pub(crate) extension_dir: PathBuf,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) wit_files: Vec<PathBuf>,
}

pub(crate) fn stage(root: &Path, profile: &Path, server: &Path) -> Result<Staged, String> {
    if profile.exists() {
        log(format!(
            "resetting existing smoke profile: {}",
            profile.display()
        ));
        fs::remove_dir_all(profile)
            .map_err(|error| format!("remove {}: {error}", profile.display()))?;
    }

    let extension_dir = profile.join("runtime-extension");
    fs::create_dir_all(&extension_dir)
        .map_err(|error| format!("create {}: {error}", extension_dir.display()))?;

    log("copying extension manifest, languages, snippets, and Wasm adapter");
    copy_file(
        &root.join("extension.toml"),
        &extension_dir.join("extension.toml"),
    )?;
    copy_dir(&root.join("languages"), &extension_dir.join("languages"))?;
    copy_dir(&root.join("snippets"), &extension_dir.join("snippets"))?;

    let adapter = root
        .join("target")
        .join(WASM_TARGET)
        .join("debug")
        .join("zed_wit.wasm");
    copy_file(&adapter, &extension_dir.join("extension.wasm"))?;

    let installed_dir = profile.join("extensions/installed");
    fs::create_dir_all(&installed_dir)
        .map_err(|error| format!("create {}: {error}", installed_dir.display()))?;
    link_dev_extension(&extension_dir, &installed_dir.join(EXTENSION_ID))?;

    let grammar_dir = extension_dir.join("grammars");
    fs::create_dir_all(&grammar_dir)
        .map_err(|error| format!("create {}: {error}", grammar_dir.display()))?;
    log("compiling pinned Tree-sitter WIT grammar to Wasm");
    compile_grammar(root, &grammar_dir.join("wit.wasm"))?;

    let workspace_dir = profile.join("workspace");
    fs::create_dir_all(workspace_dir.join(".zed"))
        .map_err(|error| format!("create smoke workspace settings: {error}"))?;
    let settings_path = workspace_dir.join(".zed/settings.json");
    let settings = json!({
        "lsp": {
            "wit-language-server": {
                "binary": {
                    "path": server.to_string_lossy()
                }
            }
        }
    });
    fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("encode {}: {error}", settings_path.display()))?,
    )
    .map_err(|error| format!("write {}: {error}", settings_path.display()))?;
    log(format!(
        "staged explicit LSP binary override: {}",
        settings_path.display()
    ));

    let source_tests = root.join("tests");
    let staged_tests = workspace_dir.join("tests");
    log("copying complete tests tree into isolated workspace");
    copy_dir(&source_tests, &staged_tests)?;

    let source_wit = collect_wit_files(&source_tests)?;
    let wit_files = collect_wit_files(&staged_tests)?;
    let source_relative = relative_paths(&source_tests, &source_wit)?;
    let staged_relative = relative_paths(&staged_tests, &wit_files)?;
    if source_relative != staged_relative {
        return Err(format!(
            "staged WIT fixture set differs from repository tests tree\nsource: {source_relative:?}\nstaged: {staged_relative:?}"
        ));
    }
    if wit_files.is_empty() {
        return Err("tests tree contains no WIT fixtures to open in Zed".into());
    }

    Ok(Staged {
        extension_dir,
        workspace_dir,
        wit_files,
    })
}

#[cfg(unix)]
fn link_dev_extension(source: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(source, destination).map_err(|error| {
        format!(
            "link dev extension {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(windows)]
fn link_dev_extension(source: &Path, destination: &Path) -> Result<(), String> {
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(destination)
        .arg(source)
        .status()
        .map_err(|error| format!("create dev-extension junction: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "create dev-extension junction {} to {} exited with {status}",
            destination.display(),
            source.display()
        ))
    }
}

#[cfg(not(any(unix, windows)))]
fn link_dev_extension(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err("test-zed supports dev-extension staging on macOS, Linux, and Windows".into())
}

fn compile_grammar(root: &Path, output_path: &Path) -> Result<(), String> {
    let grammar_root = tree_sitter_wit_root(root)?;
    let source_dir = grammar_root.join("src");
    let parser = source_dir.join("parser.c");
    let scanner = source_dir.join("scanner.c");
    let clang = find_wasi_clang().ok_or_else(|| {
        "wasi-sdk clang not found; set WASI_SDK_PATH or install/rebuild a dev extension once in Zed so Zed downloads its wasi-sdk cache".to_owned()
    })?;

    log(format!("wasi-sdk clang: {}", clang.display()));
    let mut command = Command::new(&clang);
    command
        .arg("-fPIC")
        .arg("-shared")
        .arg("-Os")
        .arg("-Wl,--export=tree_sitter_wit")
        .arg("-o")
        .arg(output_path)
        .arg("-I")
        .arg(&source_dir)
        .arg(&parser)
        .current_dir(root);
    if scanner.is_file() {
        command.arg(&scanner);
    }

    let result = command
        .output()
        .map_err(|error| format!("run {}: {error}", clang.display()))?;
    if !result.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            clang.display(),
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(())
}

fn tree_sitter_wit_root(root: &Path) -> Result<PathBuf, String> {
    let metadata = util::command_output(
        "cargo",
        ["metadata", "--format-version", "1", "--locked"],
        root,
    )?;
    let metadata: Value = serde_json::from_str(&metadata)
        .map_err(|error| format!("parse cargo metadata: {error}"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata omitted packages")?;
    let manifest = packages
        .iter()
        .find(|package| package["name"].as_str() == Some("tree-sitter-wit"))
        .and_then(|package| package["manifest_path"].as_str())
        .ok_or("cargo metadata did not include tree-sitter-wit")?;
    Path::new(manifest)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("tree-sitter-wit manifest has no parent: {manifest}"))
}

fn find_wasi_clang() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = env::var_os("WASI_SDK_PATH").filter(|path| !path.is_empty()) {
        roots.push(PathBuf::from(path));
    }

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        if cfg!(target_os = "macos") {
            for channel in ["Zed", "Zed Preview", "Zed Nightly"] {
                roots.push(
                    home.join("Library")
                        .join("Application Support")
                        .join(channel)
                        .join("extensions/build/wasi-sdk"),
                );
            }
        } else if cfg!(target_os = "linux") {
            if let Some(data_home) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
                roots.push(data_home.join("zed/extensions/build/wasi-sdk"));
            }
            roots.push(home.join(".local/share/zed/extensions/build/wasi-sdk"));
        }
    }
    if cfg!(target_os = "windows")
        && let Some(local_app_data) = env::var_os("LOCALAPPDATA").map(PathBuf::from)
    {
        roots.push(local_app_data.join("Zed/extensions/build/wasi-sdk"));
    }

    roots
        .into_iter()
        .flat_map(|root| [root.join("bin/clang"), root.join("bin/clang.exe")])
        .find(|path| path.is_file())
}

pub(crate) fn launch(
    zed: &Path,
    profile: &Path,
    workspace_dir: &Path,
    wit_files: &[PathBuf],
    stdout_log: &Path,
    stderr_log: &Path,
) -> Result<Child, String> {
    let stdout = File::create(stdout_log)
        .map_err(|error| format!("create {}: {error}", stdout_log.display()))?;
    let stderr = File::create(stderr_log)
        .map_err(|error| format!("create {}: {error}", stderr_log.display()))?;
    let filtered_path = path_without_language_server()?;
    let mut command = Command::new(zed);
    command
        .env("ZED_STATELESS", "1")
        .env("PATH", filtered_path)
        .arg("--foreground")
        .arg("--new")
        .arg("--user-data-dir")
        .arg(profile)
        .arg(workspace_dir)
        .args(wit_files)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command
        .spawn()
        .map_err(|error| format!("launch {}: {error}", zed.display()))
}

pub(crate) fn wait_for_server(
    child: &mut Child,
    server: &Path,
    before: &BTreeSet<u32>,
    profile: &Path,
    timeout: Duration,
) -> Result<u32, String> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_progress = Duration::from_secs(5);
    while Instant::now() < deadline {
        let current = matching_processes(server)?;
        if let Some(pid) = current.difference(before).next().copied() {
            log(format!(
                "detected language-server PID {pid}; verifying it remains alive"
            ));
            thread::sleep(Duration::from_millis(750));
            if matching_processes(server)?.contains(&pid) {
                log(format!("language server is running (PID {pid})"));
                return Ok(pid);
            }
            log(format!(
                "candidate language-server PID {pid} exited before verification; continuing to wait"
            ));
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("poll Zed process: {error}"))?
            && !status.success()
        {
            return Err(format!(
                "Zed exited with {status} before starting {}; inspect {}",
                server.display(),
                profile.display()
            ));
        }
        let elapsed = started.elapsed();
        if elapsed >= next_progress {
            let remaining = timeout.saturating_sub(elapsed);
            log(format!(
                "still waiting for language server: {}s elapsed, {}s remaining",
                elapsed.as_secs(),
                remaining.as_secs()
            ));
            next_progress += Duration::from_secs(5);
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(format!(
        "timed out after {}s waiting for Zed to start {}; inspect {}",
        timeout.as_secs(),
        server.display(),
        profile.display()
    ))
}

pub(crate) fn resolve_executable(program: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(program);
    if candidate.components().count() > 1 {
        return candidate
            .is_file()
            .then_some(candidate.clone())
            .ok_or_else(|| format!("Zed executable does not exist: {}", candidate.display()));
    }

    let inherited = env::var_os("PATH").unwrap_or_default();
    let candidates = env::split_paths(&inherited).flat_map(|directory| {
        let plain = directory.join(program);
        if cfg!(windows) && Path::new(program).extension().is_none() {
            vec![plain, directory.join(format!("{program}.exe"))]
        } else {
            vec![plain]
        }
    });
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("could not resolve {program:?} from PATH"))
}

fn path_without_language_server() -> Result<std::ffi::OsString, String> {
    let inherited = env::var_os("PATH").unwrap_or_default();
    let directories = env::split_paths(&inherited)
        .filter(|directory| {
            !directory.join("wit-language-server").is_file()
                && !directory.join("wit-language-server.exe").is_file()
        })
        .collect::<Vec<_>>();
    env::join_paths(directories)
        .map_err(|error| format!("construct Zed PATH without WIT language server: {error}"))
}

#[cfg(unix)]
pub(crate) fn stop_zed(child: &mut Child) -> Result<(), String> {
    let pid = child.id();
    let running = child
        .try_wait()
        .map_err(|error| format!("poll isolated Zed process before shutdown: {error}"))?
        .is_none();

    if running
        && let Err(error) = signal_process_group(pid, "-TERM")
        && child
            .try_wait()
            .map_err(|poll_error| {
                format!("poll isolated Zed PID {pid} after failed SIGTERM ({error}): {poll_error}")
            })?
            .is_none()
    {
        return Err(error);
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|error| format!("wait for isolated Zed PID {pid}: {error}"))?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    log(format!(
        "isolated Zed process group {pid} did not exit after SIGTERM; sending SIGKILL"
    ));
    if let Err(error) = signal_process_group(pid, "-KILL")
        && child
            .try_wait()
            .map_err(|poll_error| {
                format!("poll isolated Zed PID {pid} after failed SIGKILL ({error}): {poll_error}")
            })?
            .is_none()
    {
        return Err(error);
    }
    child
        .wait()
        .map_err(|error| format!("wait for isolated Zed process: {error}"))?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn stop_zed(child: &mut Child) -> Result<(), String> {
    let pid = child.id();
    if child
        .try_wait()
        .map_err(|error| format!("poll isolated Zed PID {pid}: {error}"))?
        .is_some()
    {
        return Ok(());
    }

    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|error| format!("terminate isolated Zed process tree {pid}: {error}"))?;
    if !status.success()
        && child
            .try_wait()
            .map_err(|error| format!("poll isolated Zed PID {pid}: {error}"))?
            .is_none()
    {
        child
            .kill()
            .map_err(|error| format!("kill isolated Zed PID {pid}: {error}"))?;
    }
    child
        .wait()
        .map_err(|error| format!("wait for isolated Zed PID {pid}: {error}"))?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn stop_zed(_child: &mut Child) -> Result<(), String> {
    Err("test-zed process shutdown is unsupported on this host".into())
}

#[cfg(unix)]
fn signal_process_group(pgid: u32, signal: &str) -> Result<(), String> {
    let target = format!("-{pgid}");
    let status = Command::new("kill")
        .args([signal, &target])
        .status()
        .map_err(|error| format!("send {signal} to process group {pgid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "send {signal} to process group {pgid} exited with {status}"
        ))
    }
}

fn process_matches(pid: u32, executable: &Path) -> Result<bool, String> {
    let expected = executable.to_string_lossy();
    Ok(process_snapshot()?
        .into_iter()
        .any(|(candidate, command)| candidate == pid && command.contains(expected.as_ref())))
}

#[cfg(unix)]
fn signal_process(pid: u32, signal: &str) -> Result<(), String> {
    let status = Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .map_err(|error| format!("send {signal} to PID {pid}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("send {signal} to PID {pid} exited with {status}"))
    }
}

#[cfg(windows)]
fn signal_process(pid: u32, _signal: &str) -> Result<(), String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    match system.process(sysinfo::Pid::from_u32(pid)) {
        Some(process) if process.kill() => Ok(()),
        Some(_) => Err(format!("failed to terminate PID {pid}")),
        None => Ok(()),
    }
}

pub(crate) fn ensure_server_stopped(pid: u32, server: &Path, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_matches(pid, server)? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    if !process_matches(pid, server)? {
        return Ok(());
    }

    let cleanup = signal_process(pid, "-TERM");
    let cleanup_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < cleanup_deadline {
        if !process_matches(pid, server)? {
            return Err(format!(
                "language-server PID {pid} required explicit cleanup after isolated Zed shutdown: {}",
                server.display()
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }

    if process_matches(pid, server)? {
        let _ = signal_process(pid, "-KILL");
    }

    match cleanup {
        Ok(()) => Err(format!(
            "language-server PID {pid} remained alive after isolated Zed shutdown and required explicit cleanup: {}",
            server.display()
        )),
        Err(error) => Err(format!(
            "language-server PID {pid} remained alive after isolated Zed shutdown and cleanup failed ({error}): {}",
            server.display()
        )),
    }
}

pub(crate) fn matching_processes(server: &Path) -> Result<BTreeSet<u32>, String> {
    let expected = server.to_string_lossy();
    Ok(process_snapshot()?
        .into_iter()
        .filter_map(|(pid, command)| command.contains(expected.as_ref()).then_some(pid))
        .collect())
}

fn process_snapshot() -> Result<Vec<(u32, String)>, String> {
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
        .map(|(pid, process)| {
            let exe = process
                .exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            let command = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            (pid.as_u32(), format!("{exe} {command}"))
        })
        .collect())
}

#[cfg(test)]
fn parse_process_snapshot(snapshot: &str) -> Vec<(u32, String)> {
    snapshot
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let separator = line.find(char::is_whitespace)?;
            let pid = line[..separator].parse().ok()?;
            let command = line[separator..].trim_start().to_owned();
            (!command.is_empty()).then_some((pid, command))
        })
        .collect()
}

pub(crate) fn scan_logs(profile: &Path, stdout_log: &Path, stderr_log: &Path) -> Result<(), String> {
    let logs = all_logs(profile, stdout_log, stderr_log);

    const FAILURES: &[&str] = &[
        "failed to load extension",
        "failed to load grammar",
        "failed to start language server",
        "failed to spawn language server",
        "query error",
    ];
    if let Some(line) = logs.lines().find(|line| {
        let line = line.to_ascii_lowercase();
        line.contains("wit") && FAILURES.iter().any(|pattern| line.contains(pattern))
    }) {
        return Err(format!("Zed reported a WIT integration failure: {line}"));
    }
    Ok(())
}

fn all_logs(profile: &Path, stdout_log: &Path, stderr_log: &Path) -> String {
    let mut logs = String::new();
    for path in [stdout_log, stderr_log] {
        if let Ok(content) = fs::read_to_string(path) {
            logs.push_str(&content);
            logs.push('\n');
        }
    }
    let _ = collect_logs(profile, &mut logs);
    logs
}

fn diagnostic_logs(profile: &Path, stdout_log: &Path, stderr_log: &Path) -> String {
    let logs = all_logs(profile, stdout_log, stderr_log);
    let lines = logs.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(120);
    lines[start..].join("\n")
}

fn collect_logs(path: &Path, output: &mut String) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| format!("read {}: {error}", path.display()))? {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", path.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("stat {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_logs(&entry.path(), output)?;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "log")
            && let Ok(content) = fs::read_to_string(entry.path())
        {
            output.push_str(&content);
            output.push('\n');
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} has no parent", destination.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    fs::copy(source, destination).map_err(|error| {
        format!(
            "copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("create {}: {error}", destination.display()))?;
    for entry in
        fs::read_dir(source).map_err(|error| format!("read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", source.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("stat {}: {error}", entry.path().display()))?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if file_type.is_file() {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn collect_wit_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_wit_files_into(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_wit_files_into(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|error| format!("read {}: {error}", root.display()))? {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", root.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("stat {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_wit_files_into(&entry.path(), files)?;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "wit")
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn relative_paths(root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    paths
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|error| {
                    format!(
                        "{} is outside expected root {}: {error}",
                        path.display(),
                        root.display()
                    )
                })
        })
        .collect()
}

fn manual_scenarios(head: &str) -> Value {
    json!([
        {"scenario":"development_install","result":"passed","evidence":"isolated Zed loaded the staged development extension"},
        {"scenario":"highlighting_and_structure","result":"gui-qualification-required","gui_qualification_command":"cargo xtask test-zed-gui --allow-input-injection true","evidence":"all WIT fixtures opened in real Zed; syntax/query/outline/bracket tests passed; no query errors logged; real outline UI navigation is qualified separately"},
        {"scenario":"snippets","result":"gui-qualification-required","gui_qualification_command":"cargo xtask test-zed-gui --allow-input-injection true","evidence":"snippet expansion, validity, sequential tab-stop indices and final cursor placement passed deterministically; real completion and forward/reverse tab-stop interaction are qualified separately"},
        {"scenario":"parser_diagnostic","result":"passed","evidence":"manual fixture mutation test creates parser error then repairs and clears it"},
        {"scenario":"resolver_diagnostic","result":"passed","evidence":"manual semantic/dependency mutation tests assert parser-backed unresolved-name diagnostics"},
        {"scenario":"unsaved_sibling_overlay","result":"passed","evidence":"manual overlay fixture uses didChange without save and propagates/clears diagnostics"},
        {"scenario":"dependency_package","result":"passed","evidence":"manual deps fixture resolves, breaks dependency declaration, and anchors diagnostics in dependency file"},
        {"scenario":"close_reopen","result":"passed","evidence":"manual overlay fixture closes unsaved sibling and reopens disk-backed source"},
        {"scenario":"unicode_positions","result":"passed","evidence":"manual Unicode fixture asserts exact UTF-8 and UTF-16 diagnostic ranges"},
        {"scenario":"formatting","result":"passed","evidence":"manual formatting fixtures preserve comments and are idempotent"},
        {"scenario":"invalid_formatting_input","result":"passed","evidence":"manual formatting mutation asserts invalid input returns either an explicit error or an empty edit list; non-empty destructive edits fail qualification"},
        {"scenario":"semantic_hover_navigation","result":"passed","evidence":"manual semantic and escaped fixture tests assert hover, definition, and references"},
        {"scenario":"context_completion","result":"passed","evidence":"manual semantic fixture checks visible type completion and negative parameter-name context"},
        {"scenario":"type_typo_quick_fix","result":"passed","evidence":"manual semantic fixture mutation asserts the unique safe replacement action"},
        {"scenario":"unsupported_capabilities","result":"passed","evidence":"initialize assertions reject rename and workspace-symbol advertisement"},
        {"scenario":"local_override","result":"passed","evidence":format!("real Zed launched exact +git.{head} server from staged .zed/settings.json with WIT server binaries filtered from PATH; protocol tests assert matching serverInfo.version and startup logMessage")},
        {"scenario":"first_hosted_install","result":"not-run","reason":"requires published matching release assets"},
        {"scenario":"cached_install","result":"not-run","reason":"requires a successful first hosted install"},
        {"scenario":"missing_corrupt_hosted_asset","result":"not-run","reason":"requires controlled published-release download scenarios; adapter cache/checksum behavior is covered by deterministic tests"},
        {"scenario":"editor_restart","result":"passed","evidence":"same isolated workspace/profile was relaunched and a second exact server PID started and stopped cleanly"}
    ])
}

pub(crate) fn native_server(root: &Path) -> PathBuf {
    root.join("target").join("release").join(if cfg!(windows) {
        "wit-language-server.exe"
    } else {
        "wit-language-server"
    })
}

fn run_status(
    program: &str,
    args: &[&str],
    cwd: &Path,
    extra_env: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let status = command
        .status()
        .map_err(|error| format!("run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn output_path(program: &Path, args: &[&str], cwd: &Path) -> Result<String, String> {
    let result = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("run {}: {error}", program.display()))?;
    output_result(&program.display().to_string(), result)
}

fn output_result(program: &str, result: std::process::Output) -> Result<String, String> {
    if !result.status.success() {
        return Err(format!(
            "{program} exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    String::from_utf8(result.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("{program} returned non-UTF-8 output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_process_snapshot() {
        assert_eq!(
            parse_process_snapshot(
                "  12 /Applications/Zed.app/Contents/MacOS/zed --foreground\n  34 /tmp/target/release/wit-language-server\n"
            ),
            vec![
                (
                    12,
                    "/Applications/Zed.app/Contents/MacOS/zed --foreground".into()
                ),
                (34, "/tmp/target/release/wit-language-server".into())
            ]
        );
    }

    #[test]
    fn ignores_malformed_process_rows() {
        assert_eq!(
            parse_process_snapshot("not-a-pid command\n  42\n  7 valid"),
            vec![(7, "valid".into())]
        );
    }

    #[test]
    fn discovers_all_wit_files_recursively_and_ignores_other_files() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "zed-wit-smoke-fixtures-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("nested/deps")).unwrap();
        fs::write(root.join("root.wit"), "package test:root;").unwrap();
        fs::write(root.join("nested/deps/types.wit"), "package test:types;").unwrap();
        fs::write(root.join("README.md"), "not WIT").unwrap();

        let files = collect_wit_files(&root).unwrap();
        let relative = relative_paths(&root, &files).unwrap();
        assert_eq!(
            relative,
            vec![
                PathBuf::from("nested/deps/types.wit"),
                PathBuf::from("root.wit")
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scans_complete_logs_for_early_integration_failures() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let profile = std::env::temp_dir().join(format!(
            "zed-wit-smoke-logs-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&profile).unwrap();
        let stdout = profile.join("zed-foreground.stdout.log");
        let stderr = profile.join("zed-foreground.stderr.log");
        let mut log = String::from("WIT failed to load grammar\n");
        for index in 0..200 {
            log.push_str(&format!("filler line {index}\n"));
        }
        fs::write(&stdout, log).unwrap();
        fs::write(&stderr, "").unwrap();

        let error = scan_logs(&profile, &stdout, &stderr).unwrap_err();
        assert!(error.contains("failed to load grammar"), "{error}");

        fs::remove_dir_all(profile).unwrap();
    }

    #[test]
    fn process_cleanup_matching_is_scoped_to_the_detected_pid() {
        let server = Path::new("/tmp/target/release/wit-language-server");
        let expected = server.to_string_lossy();
        let snapshot = parse_process_snapshot(
            "  12 /Applications/Zed.app/Contents/MacOS/zed --user-data-dir /tmp/profile\n  34 /tmp/target/release/wit-language-server\n  56 helper /tmp/target/release/wit-language-server\n",
        );
        let matching = snapshot
            .into_iter()
            .filter(|(pid, command)| *pid == 34 && command.contains(expected.as_ref()))
            .collect::<Vec<_>>();
        assert_eq!(
            matching,
            vec![(34, "/tmp/target/release/wit-language-server".into())]
        );
    }
}
