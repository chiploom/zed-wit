use std::{path::Path, time::Duration};

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod supported {
    use crate::{util, zed_smoke};
    use serde_json::json;
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const SNIPPET_FILE: &str = "gui/snippet.wit";
    const OUTLINE_FILE: &str = "gui/outline.wit";

    struct InputDriver {
        backend: &'static str,
        executable: PathBuf,
    }

    struct SessionEvidence {
        backend: &'static str,
        server_pid: u32,
        stdout: PathBuf,
        stderr: PathBuf,
        snippet: PathBuf,
        outline: PathBuf,
    }

    pub(super) fn run(
        zed: &str,
        profile: &Path,
        timeout: Duration,
        settle: Duration,
        allow_input: bool,
        linux_backend: &str,
    ) -> Result<(), String> {
        if !allow_input {
            return Err(
                "test-zed-gui injects real keyboard input; rerun with --allow-input-injection true after saving or closing unrelated foreground applications".into(),
            );
        }

        validate_linux_backend_option(linux_backend)?;
        let root = util::repo_root();
        let head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
        let drivers = prepare_input_drivers(&root, profile, linux_backend)?;
        let drivers = preflight_input_drivers(drivers)?;
        if drivers.is_empty() {
            return Err(format!(
                "no GUI input backend passed preflight{}",
                input_permission_hint()
            ));
        }

        eprintln!(
            "[test-zed-gui] input backend preflight passed; running deterministic and real-Zed qualification"
        );
        zed_smoke::run(zed, &profile.join("smoke"), timeout)?;

        let zed_path = zed_smoke::resolve_executable(zed)?;
        let server = zed_smoke::native_server(&root);
        let mut failures = Vec::new();

        for (attempt, driver) in drivers.iter().enumerate() {
            eprintln!(
                "[test-zed-gui] GUI attempt {} using {} input backend",
                attempt + 1,
                driver.backend
            );
            match run_gui_attempt(
                &root, &zed_path, &server, profile, driver, attempt, timeout, settle,
            ) {
                Ok(evidence) => {
                    write_report(profile, &head, command_version(&zed_path)?, &evidence)?;
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("[test-zed-gui] {} backend failed: {error}", driver.backend);
                    failures.push(format!("{}: {error}", driver.backend));
                }
            }
        }

        Err(format!(
            "all GUI input backends failed{}:\n{}",
            input_permission_hint(),
            failures.join("\n")
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn run_gui_attempt(
        root: &Path,
        zed: &Path,
        server: &Path,
        profile: &Path,
        driver: &InputDriver,
        attempt: usize,
        timeout: Duration,
        settle: Duration,
    ) -> Result<SessionEvidence, String> {
        let gui_profile = profile.join(format!("interactive-{attempt}-{}", driver.backend));
        let staged = zed_smoke::stage(root, &gui_profile, server)?;
        write_isolated_config(&gui_profile)?;

        let gui_dir = staged.workspace_dir.join("gui");
        fs::create_dir_all(&gui_dir)
            .map_err(|error| format!("create {}: {error}", gui_dir.display()))?;
        let snippet = staged.workspace_dir.join(SNIPPET_FILE);
        let outline = staged.workspace_dir.join(OUTLINE_FILE);
        fs::write(&snippet, "")
            .map_err(|error| format!("write {}: {error}", snippet.display()))?;
        fs::write(&outline, outline_fixture())
            .map_err(|error| format!("write {}: {error}", outline.display()))?;

        let stdout = gui_profile.join("zed-gui.stdout.log");
        let stderr = gui_profile.join("zed-gui.stderr.log");
        let before = zed_smoke::matching_processes(server)?;
        let mut child = zed_smoke::launch(
            zed,
            &gui_profile,
            &staged.workspace_dir,
            &[snippet.clone()],
            &stdout,
            &stderr,
        )?;

        let server_pid =
            match zed_smoke::wait_for_server(&mut child, server, &before, &gui_profile, timeout) {
                Ok(pid) => pid,
                Err(error) => {
                    let _ = zed_smoke::stop_zed(&mut child);
                    return Err(error);
                }
            };

        thread::sleep(settle);
        let input_result = run_input_driver(driver, settle, timeout);
        let stop_result = zed_smoke::stop_zed(&mut child);
        let server_stop_result =
            zed_smoke::ensure_server_stopped(server_pid, server, Duration::from_secs(3));
        let log_result = zed_smoke::scan_logs(&gui_profile, &stdout, &stderr);

        let mut cleanup_errors = Vec::new();
        if let Err(error) = stop_result {
            cleanup_errors.push(format!("stop isolated Zed: {error}"));
        }
        if let Err(error) = server_stop_result {
            cleanup_errors.push(format!("stop exact language server: {error}"));
        }
        if let Err(error) = log_result {
            cleanup_errors.push(format!("scan isolated Zed logs: {error}"));
        }

        if let Err(error) = input_result {
            if cleanup_errors.is_empty() {
                return Err(format!("GUI input helper failed: {error}"));
            }
            return Err(format!(
                "GUI input helper failed: {error}; cleanup/integration errors: {}",
                cleanup_errors.join("; ")
            ));
        }
        if !cleanup_errors.is_empty() {
            return Err(format!(
                "GUI cleanup/integration errors: {}",
                cleanup_errors.join("; ")
            ));
        }

        verify_snippet(&snippet)?;
        verify_outline(&outline)?;

        Ok(SessionEvidence {
            backend: driver.backend,
            server_pid,
            stdout,
            stderr,
            snippet,
            outline,
        })
    }

    fn prepare_input_drivers(
        root: &Path,
        profile: &Path,
        linux_backend: &str,
    ) -> Result<Vec<InputDriver>, String> {
        let backends = selected_backend_features(linux_backend)?;
        backends
            .into_iter()
            .map(|(backend, feature)| {
                let target_dir = profile.join("input-drivers").join(backend);
                if target_dir.exists() {
                    fs::remove_dir_all(&target_dir)
                        .map_err(|error| format!("remove {}: {error}", target_dir.display()))?;
                }
                fs::create_dir_all(&target_dir)
                    .map_err(|error| format!("create {}: {error}", target_dir.display()))?;

                eprintln!("[test-zed-gui] building {backend} input helper with feature {feature}");
                let status = Command::new("cargo")
                    .args([
                        "build",
                        "-p",
                        "zed-gui-input",
                        "--locked",
                        "--features",
                        feature,
                        "--target-dir",
                    ])
                    .arg(&target_dir)
                    .current_dir(root)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .map_err(|error| format!("build {backend} input helper: {error}"))?;
                if !status.success() {
                    return Err(format!(
                        "building {backend} input helper exited with {status}"
                    ));
                }

                let executable = target_dir.join("debug").join(if cfg!(windows) {
                    "zed-gui-input.exe"
                } else {
                    "zed-gui-input"
                });
                if !executable.is_file() {
                    return Err(format!(
                        "{backend} input helper was not produced at {}",
                        executable.display()
                    ));
                }
                Ok(InputDriver {
                    backend,
                    executable,
                })
            })
            .collect()
    }

    fn preflight_input_drivers(drivers: Vec<InputDriver>) -> Result<Vec<InputDriver>, String> {
        let mut available = Vec::new();
        let mut failures = Vec::new();
        for driver in drivers {
            match probe_input_driver(&driver) {
                Ok(()) => {
                    eprintln!(
                        "[test-zed-gui] {} input backend preflight PASS",
                        driver.backend
                    );
                    available.push(driver);
                }
                Err(error) => {
                    eprintln!(
                        "[test-zed-gui] {} input backend preflight failed: {error}",
                        driver.backend
                    );
                    failures.push(format!("{}: {error}", driver.backend));
                }
            }
        }

        if available.is_empty() {
            Err(format!(
                "all GUI input backend preflights failed{}:\n{}",
                input_permission_hint(),
                failures.join("\n")
            ))
        } else {
            Ok(available)
        }
    }

    fn probe_input_driver(driver: &InputDriver) -> Result<(), String> {
        let output = Command::new(&driver.executable)
            .arg("probe")
            .output()
            .map_err(|error| {
                format!(
                    "launch {} input preflight {}: {error}",
                    driver.backend,
                    driver.executable.display()
                )
            })?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit status {}", output.status)
        };
        Err(detail)
    }

    fn selected_backend_features(
        linux_backend: &str,
    ) -> Result<Vec<(&'static str, &'static str)>, String> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if linux_backend != "auto" {
                return Err("--linux-input-backend is only valid on Linux".into());
            }
            return Ok(vec![("native", "native")]);
        }

        #[cfg(target_os = "linux")]
        {
            return match linux_backend {
                "x11" => Ok(vec![("x11", "linux-x11")]),
                "wayland" => Ok(vec![("wayland", "linux-wayland")]),
                "libei" => Ok(vec![("libei", "linux-libei")]),
                "auto" => {
                    let session = env::var("XDG_SESSION_TYPE")
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if session == "x11"
                        || (session.is_empty()
                            && env::var_os("WAYLAND_DISPLAY").is_none()
                            && env::var_os("DISPLAY").is_some())
                    {
                        Ok(vec![("x11", "linux-x11")])
                    } else if session == "wayland" || env::var_os("WAYLAND_DISPLAY").is_some() {
                        Ok(vec![("libei", "linux-libei"), ("wayland", "linux-wayland")])
                    } else {
                        Err(
                            "cannot infer Linux desktop input backend; set XDG_SESSION_TYPE/DISPLAY/WAYLAND_DISPLAY or pass --linux-input-backend x11|wayland|libei"
                                .into(),
                        )
                    }
                }
                other => Err(format!(
                    "invalid --linux-input-backend {other:?}; expected auto, x11, wayland, or libei"
                )),
            };
        }

        #[allow(unreachable_code)]
        Err("GUI input backend is unsupported on this host".into())
    }

    fn validate_linux_backend_option(value: &str) -> Result<(), String> {
        if matches!(value, "auto" | "x11" | "wayland" | "libei") {
            Ok(())
        } else {
            Err(format!(
                "invalid --linux-input-backend {value:?}; expected auto, x11, wayland, or libei"
            ))
        }
    }

    fn run_input_driver(
        driver: &InputDriver,
        settle: Duration,
        timeout: Duration,
    ) -> Result<(), String> {
        let settle_ms = u64::try_from(settle.as_millis())
            .map_err(|_| "settle duration does not fit in u64".to_owned())?;
        let settle_ms = settle_ms.to_string();
        let mut child = Command::new(&driver.executable)
            .args(["all", settle_ms.as_str()])
            .spawn()
            .map_err(|error| {
                format!(
                    "launch {} input helper {}: {error}",
                    driver.backend,
                    driver.executable.display()
                )
            })?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("poll {} input helper: {error}", driver.backend))?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} input helper exited with {status}",
                        driver.backend
                    ))
                };
            }
            thread::sleep(Duration::from_millis(100));
        }

        child
            .kill()
            .map_err(|error| format!("kill timed-out {} input helper: {error}", driver.backend))?;
        let _ = child.wait();
        Err(format!(
            "{} input helper timed out after {}s",
            driver.backend,
            timeout.as_secs()
        ))
    }

    fn verify_snippet(path: &Path) -> Result<(), String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let expected = "package gui:reverse@2.0.0;";
        if !source.contains(expected) {
            return Err(format!(
                "snippet GUI qualification did not produce {expected:?}: {source:?}"
            ));
        }
        if !source.contains("// GUI_SNIPPET_FINAL") {
            return Err(format!(
                "snippet final tab stop was not reached: {source:?}"
            ));
        }
        for unexpected in ["wit-package", "example", "snippet", "1.2.3"] {
            if source.contains(unexpected) {
                return Err(format!(
                    "snippet GUI qualification left stale placeholder content {unexpected:?}: {source:?}"
                ));
            }
        }
        Ok(())
    }

    fn verify_outline(path: &Path) -> Result<(), String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        for (declaration, marker) in [
            ("record alpha", "GUI_OUTLINE_ALPHA"),
            ("variant beta", "GUI_OUTLINE_BETA"),
            ("resource gamma", "GUI_OUTLINE_GAMMA"),
        ] {
            let line = source
                .lines()
                .find(|line| line.contains(declaration))
                .ok_or_else(|| format!("outline fixture lost declaration {declaration:?}"))?;
            if !line.contains(marker) {
                return Err(format!(
                    "outline UI did not navigate {declaration:?} to the expected line: {line:?}"
                ));
            }
        }
        Ok(())
    }

    fn outline_fixture() -> &'static str {
        "package gui:outline@1.0.0;\n\ninterface api {\n    record alpha {\n        value: string,\n    }\n\n    variant beta {\n        empty,\n        full(string),\n    }\n\n    resource gamma {\n        constructor();\n    }\n}\n"
    }

    fn write_isolated_config(profile: &Path) -> Result<(), String> {
        let config = profile.join("config");
        fs::create_dir_all(&config)
            .map_err(|error| format!("create {}: {error}", config.display()))?;

        let settings = json!({
            "accessible_mode": true,
            "vim_mode": false,
            "helix_mode": false,
            "show_completions_on_input": true,
            "snippet_sort_order": "top",
        });
        fs::write(
            config.join("settings.json"),
            serde_json::to_vec_pretty(&settings)
                .map_err(|error| format!("encode isolated GUI settings: {error}"))?,
        )
        .map_err(|error| format!("write isolated GUI settings: {error}"))?;

        Ok(())
    }

    fn write_report(
        profile: &Path,
        head: &str,
        zed_version: String,
        evidence: &SessionEvidence,
    ) -> Result<(), String> {
        let report = json!({
            "result": "passed",
            "head": head,
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "linux_session_type": env::var("XDG_SESSION_TYPE").ok(),
            "wayland_display": env::var("WAYLAND_DISPLAY").ok(),
            "x11_display": env::var("DISPLAY").ok(),
            "zed_version": zed_version,
            "input_backend": evidence.backend,
            "server_pid": evidence.server_pid,
            "stdout": evidence.stdout,
            "stderr": evidence.stderr,
            "scenarios": [
                {
                    "scenario": "snippets",
                    "result": "passed",
                    "evidence": "real Zed's default completion path expanded the WIT package snippet after typing its exact prefix; default Enter, Tab and Shift-Tab bindings replaced the expected placeholders; the final cursor accepted a sentinel and the disposable file was saved and verified",
                    "file": evidence.snippet,
                },
                {
                    "scenario": "highlighting_and_structure",
                    "result": "passed",
                    "evidence": "real Zed's default outline shortcut located record, variant, and resource symbols; each navigation target was marked and saved; deterministic query tests remain the source of truth for semantic highlight captures",
                    "file": evidence.outline,
                }
            ],
            "presentation_note": "Theme-specific pixel colors are intentionally not screenshot-compared; semantic capture correctness is deterministic and real GUI qualification verifies language activation, snippet interaction, and outline navigation.",
        });
        let report_path = profile.join("zed-gui-report.json");
        fs::create_dir_all(profile)
            .map_err(|error| format!("create {}: {error}", profile.display()))?;
        fs::write(
            &report_path,
            serde_json::to_vec_pretty(&report)
                .map_err(|error| format!("encode GUI report: {error}"))?,
        )
        .map_err(|error| format!("write {}: {error}", report_path.display()))?;

        eprintln!("[test-zed-gui] report: {}", report_path.display());
        eprintln!(
            "[test-zed-gui] PASS: snippet interaction and outline navigation passed in real Zed using {}",
            evidence.backend
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("encode GUI report: {error}"))?
        );
        Ok(())
    }

    fn command_version(zed: &Path) -> Result<String, String> {
        let output = Command::new(zed)
            .arg("--version")
            .output()
            .map_err(|error| format!("run {} --version: {error}", zed.display()))?;
        if !output.status.success() {
            return Err(format!(
                "{} --version exited with {}: {}",
                zed.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|error| format!("decode Zed version: {error}"))
    }

    fn input_permission_hint() -> &'static str {
        if cfg!(target_os = "macos") {
            "; grant Accessibility permission to the terminal/runner that invokes xtask"
        } else if cfg!(target_os = "windows") {
            "; keep Zed and xtask at the same Windows integrity level so UIPI does not block input"
        } else if cfg!(target_os = "linux") {
            "; ensure the desktop exposes the selected X11, Wayland virtual-keyboard, or libei/RemoteDesktop input interface"
        } else {
            ""
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn snippet_verification_requires_reverse_and_final_tabstop_evidence() {
            let root = env::temp_dir().join(format!("zed-wit-gui-snippet-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let path = root.join("snippet.wit");
            fs::write(&path, "package gui:reverse@2.0.0;\n// GUI_SNIPPET_FINAL\n").unwrap();
            assert!(verify_snippet(&path).is_ok());
            fs::write(&path, "package gui:snippet@1.2.3;\n// GUI_SNIPPET_FINAL\n").unwrap();
            assert!(verify_snippet(&path).is_err());
            fs::remove_dir_all(&root).unwrap();
        }

        #[test]
        fn outline_verification_requires_every_navigation_marker() {
            let root = env::temp_dir().join(format!("zed-wit-gui-outline-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let path = root.join("outline.wit");
            fs::write(
                &path,
                "record alpha { // GUI_OUTLINE_ALPHA\n}\nvariant beta { // GUI_OUTLINE_BETA\n}\nresource gamma { // GUI_OUTLINE_GAMMA\n}\n",
            )
            .unwrap();
            assert!(verify_outline(&path).is_ok());
            fs::write(
                &path,
                "record alpha {\n}\nvariant beta { // GUI_OUTLINE_BETA\n}\nresource gamma { // GUI_OUTLINE_GAMMA\n}\n",
            )
            .unwrap();
            assert!(verify_outline(&path).is_err());
            fs::remove_dir_all(&root).unwrap();
        }
    }
}

pub fn run(
    zed: &str,
    profile: &Path,
    timeout: Duration,
    settle: Duration,
    allow_input: bool,
    linux_backend: &str,
) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        supported::run(zed, profile, timeout, settle, allow_input, linux_backend)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (zed, profile, timeout, settle, allow_input, linux_backend);
        Err("test-zed-gui supports Zed desktop hosts: macOS, Linux, and Windows".into())
    }
}
