use crate::util;
use serde_json::{Value, json};
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

pub fn run(zed: &str, profile: &Path, timeout: Duration) -> Result<(), String> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err("test-zed currently supports macOS and Linux hosts".into());
    }

    let root = util::repo_root();
    let head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
    let zed_version = output(zed, &["--version"], &root)?;

    run_status(
        "cargo",
        &["test", "-p", "wit-syntax", "--test", "editing", "--locked"],
        &root,
        &[],
    )?;
    run_status(
        "cargo",
        &[
            "test",
            "-p",
            "wit-language-server",
            "--test",
            "stdio",
            "--locked",
        ],
        &root,
        &[],
    )?;
    run_status(
        "cargo",
        &["build", "--target", WASM_TARGET, "--locked"],
        &root,
        &[],
    )?;
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
    if !server_version.contains(&expected) {
        return Err(format!(
            "native server identity mismatch: expected {expected}, got {server_version:?}"
        ));
    }

    let staged = stage(&root, profile, &server)?;
    let stdout_log = profile.join("zed-foreground.stdout.log");
    let stderr_log = profile.join("zed-foreground.stderr.log");
    let before = matching_processes(&server)?;
    let mut child = launch(
        zed,
        profile,
        &staged.workspace_file,
        &stdout_log,
        &stderr_log,
    )?;

    let smoke = wait_for_server(&mut child, &server, &before, profile, timeout);
    stop_zed(&mut child, profile);
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
    scan_logs(profile, &stdout_log, &stderr_log)?;

    let report = json!({
        "result": "passed",
        "head": head,
        "zed_version": zed_version,
        "server_version": server_version,
        "profile": profile,
        "runtime_extension": staged.extension_dir,
        "workspace_file": staged.workspace_file,
        "server_pid": server_pid,
        "foreground_stdout": stdout_log,
        "foreground_stderr": stderr_log,
    });
    let report_path = profile.join("zed-smoke-report.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("encode {}: {error}", report_path.display()))?,
    )
    .map_err(|error| format!("write {}: {error}", report_path.display()))?;

    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("encode Zed smoke report: {error}"))?
    );
    Ok(())
}

struct Staged {
    extension_dir: PathBuf,
    workspace_file: PathBuf,
}

fn stage(root: &Path, profile: &Path, server: &Path) -> Result<Staged, String> {
    if profile.exists() {
        fs::remove_dir_all(profile)
            .map_err(|error| format!("remove {}: {error}", profile.display()))?;
    }

    let extension_dir = profile.join("runtime-extension");
    fs::create_dir_all(&extension_dir)
        .map_err(|error| format!("create {}: {error}", extension_dir.display()))?;

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
    compile_grammar(root, &grammar_dir.join("wit.wasm"))?;

    let workspace = profile.join("workspace");
    fs::create_dir_all(workspace.join(".zed"))
        .map_err(|error| format!("create smoke workspace: {error}"))?;
    let workspace_file = workspace.join("main.wit");
    copy_file(
        &root.join("tests/manual-zed/semantic/main.wit"),
        &workspace_file,
    )?;

    let settings = json!({
        "lsp": {
            "wit-language-server": {
                "binary": { "path": server }
            }
        }
    });
    let settings_path = workspace.join(".zed/settings.json");
    fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("encode {}: {error}", settings_path.display()))?,
    )
    .map_err(|error| format!("write {}: {error}", settings_path.display()))?;

    Ok(Staged {
        extension_dir,
        workspace_file,
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

#[cfg(not(unix))]
fn link_dev_extension(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err("test-zed currently requires Unix symlink support".into())
}

fn compile_grammar(root: &Path, output_path: &Path) -> Result<(), String> {
    let grammar_root = tree_sitter_wit_root(root)?;
    let source_dir = grammar_root.join("src");
    let parser = source_dir.join("parser.c");
    let scanner = source_dir.join("scanner.c");
    let clang = find_wasi_clang().ok_or_else(|| {
        "wasi-sdk clang not found; set WASI_SDK_PATH or install/rebuild a dev extension once in Zed so Zed downloads its wasi-sdk cache".to_owned()
    })?;

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

    roots
        .into_iter()
        .flat_map(|root| [root.join("bin/clang"), root.join("bin/clang.exe")])
        .find(|path| path.is_file())
}

fn launch(
    zed: &str,
    profile: &Path,
    workspace_file: &Path,
    stdout_log: &Path,
    stderr_log: &Path,
) -> Result<Child, String> {
    let stdout = File::create(stdout_log)
        .map_err(|error| format!("create {}: {error}", stdout_log.display()))?;
    let stderr = File::create(stderr_log)
        .map_err(|error| format!("create {}: {error}", stderr_log.display()))?;
    Command::new(zed)
        .env("ZED_STATELESS", "1")
        .arg("--foreground")
        .arg("--new")
        .arg("--user-data-dir")
        .arg(profile)
        .arg(workspace_file)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("launch {zed}: {error}"))
}

fn wait_for_server(
    child: &mut Child,
    server: &Path,
    before: &BTreeSet<u32>,
    profile: &Path,
    timeout: Duration,
) -> Result<u32, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let current = matching_processes(server)?;
        if let Some(pid) = current.difference(before).next().copied() {
            thread::sleep(Duration::from_millis(750));
            if matching_processes(server)?.contains(&pid) {
                return Ok(pid);
            }
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
        thread::sleep(Duration::from_millis(250));
    }

    Err(format!(
        "timed out after {}s waiting for Zed to start {}; inspect {}",
        timeout.as_secs(),
        server.display(),
        profile.display()
    ))
}

fn stop_zed(child: &mut Child, profile: &Path) {
    let _ = child.kill();
    let _ = child.wait();

    if let Ok(processes) = process_snapshot() {
        let profile = profile.to_string_lossy();
        for (pid, command) in processes {
            if command.contains(profile.as_ref()) {
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status();
            }
        }
    }
}

fn matching_processes(server: &Path) -> Result<BTreeSet<u32>, String> {
    let expected = server.to_string_lossy();
    Ok(process_snapshot()?
        .into_iter()
        .filter_map(|(pid, command)| command.contains(expected.as_ref()).then_some(pid))
        .collect())
}

fn process_snapshot() -> Result<Vec<(u32, String)>, String> {
    let result = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .map_err(|error| format!("run ps: {error}"))?;
    if !result.status.success() {
        return Err(format!("ps exited with {}", result.status));
    }
    let stdout =
        String::from_utf8(result.stdout).map_err(|error| format!("decode ps output: {error}"))?;
    Ok(parse_process_snapshot(&stdout))
}

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

fn scan_logs(profile: &Path, stdout_log: &Path, stderr_log: &Path) -> Result<(), String> {
    let logs = diagnostic_logs(profile, stdout_log, stderr_log);

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

fn diagnostic_logs(profile: &Path, stdout_log: &Path, stderr_log: &Path) -> String {
    let mut logs = String::new();
    for path in [stdout_log, stderr_log] {
        if let Ok(content) = fs::read_to_string(path) {
            logs.push_str(&content);
            logs.push('\n');
        }
    }
    let _ = collect_logs(profile, &mut logs);
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

fn native_server(root: &Path) -> PathBuf {
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

fn output(program: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
    let result = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("run {program}: {error}"))?;
    output_result(program, result)
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
}
