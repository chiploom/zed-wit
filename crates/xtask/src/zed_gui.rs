use std::{
    path::Path,
    time::Duration,
};

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod supported {
    use crate::{util, zed_smoke};
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    use serde_json::json;
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::{Child, Command},
        thread,
        time::Duration,
    };

    const SNIPPET_FILE: &str = "gui/snippet.wit";
    const OUTLINE_FILE: &str = "gui/outline.wit";

    pub(super) fn run(
        zed: &str,
        profile: &Path,
        timeout: Duration,
        settle: Duration,
        allow_input: bool,
    ) -> Result<(), String> {
        if !allow_input {
            return Err(
                "test-zed-gui injects real keyboard input; rerun with --allow-input-injection true after closing or saving unrelated foreground applications".into(),
            );
        }

        let root = util::repo_root();
        let head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
        let smoke_profile = profile.join("smoke");
        eprintln!("[test-zed-gui] running deterministic and real-Zed qualification first");
        zed_smoke::run(zed, &smoke_profile, timeout)?;

        let zed_path = zed_smoke::resolve_executable(zed)?;
        let server = zed_smoke::native_server(&root);
        let gui_profile = profile.join("interactive");
        let staged = zed_smoke::stage(&root, &gui_profile, &server)?;
        write_isolated_config(&gui_profile)?;

        let gui_dir = staged.workspace_dir.join("gui");
        fs::create_dir_all(&gui_dir)
            .map_err(|error| format!("create {}: {error}", gui_dir.display()))?;
        let snippet_path = staged.workspace_dir.join(SNIPPET_FILE);
        let outline_path = staged.workspace_dir.join(OUTLINE_FILE);
        fs::write(&snippet_path, "wit-package")
            .map_err(|error| format!("write {}: {error}", snippet_path.display()))?;
        fs::write(&outline_path, outline_fixture())
            .map_err(|error| format!("write {}: {error}", outline_path.display()))?;

        eprintln!("[test-zed-gui] qualifying snippet completion and tab-stop traversal");
        let snippet_evidence = run_session(
            &zed_path,
            &gui_profile,
            &staged.workspace_dir,
            &server,
            &snippet_path,
            "snippet",
            timeout,
            settle,
            |input| exercise_snippet(input, settle),
        )?;
        verify_snippet(&snippet_path)?;

        eprintln!("[test-zed-gui] qualifying outline navigation and structure");
        let outline_evidence = run_session(
            &zed_path,
            &gui_profile,
            &staged.workspace_dir,
            &server,
            &outline_path,
            "outline",
            timeout,
            settle,
            |input| exercise_outline(input, settle),
        )?;
        verify_outline(&outline_path)?;

        let zed_version = command_version(&zed_path)?;
        let report = json!({
            "result": "passed",
            "head": head,
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "linux_session_type": env::var("XDG_SESSION_TYPE").ok(),
            "wayland_display": env::var("WAYLAND_DISPLAY").ok(),
            "x11_display": env::var("DISPLAY").ok(),
            "zed_version": zed_version,
            "profile": gui_profile,
            "input_injection": {
                "library": "enigo",
                "version": "0.6.1",
                "explicitly_allowed": true,
            },
            "scenarios": [
                {
                    "scenario": "snippets",
                    "result": "passed",
                    "evidence": "real Zed completion expanded the WIT package snippet; forward and reverse snippet-tab actions changed the expected placeholders; the final cursor accepted a sentinel and the disposable file was saved and verified",
                    "file": snippet_path,
                    "server_pid": snippet_evidence.server_pid,
                    "stdout": snippet_evidence.stdout,
                    "stderr": snippet_evidence.stderr,
                },
                {
                    "scenario": "highlighting_and_structure",
                    "result": "passed",
                    "evidence": "real Zed outline UI located record, variant, and resource symbols in the WIT fixture; each navigation target was marked and saved; deterministic query tests remain the source of truth for semantic highlight captures",
                    "file": outline_path,
                    "server_pid": outline_evidence.server_pid,
                    "stdout": outline_evidence.stdout,
                    "stderr": outline_evidence.stderr,
                }
            ],
            "presentation_note": "Theme-specific pixel colors are intentionally not screenshot-compared; semantic capture correctness is deterministic and the real GUI qualification verifies Zed language activation, snippet interaction, and outline navigation.",
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
        eprintln!("[test-zed-gui] PASS: snippet interaction and outline navigation passed in real Zed");
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("encode GUI report: {error}"))?
        );
        Ok(())
    }

    struct SessionEvidence {
        server_pid: u32,
        stdout: PathBuf,
        stderr: PathBuf,
    }

    #[allow(clippy::too_many_arguments)]
    fn run_session(
        zed: &Path,
        profile: &Path,
        workspace: &Path,
        server: &Path,
        file: &Path,
        name: &str,
        timeout: Duration,
        settle: Duration,
        action: impl FnOnce(&mut Enigo) -> Result<(), String>,
    ) -> Result<SessionEvidence, String> {
        let stdout = profile.join(format!("zed-gui-{name}.stdout.log"));
        let stderr = profile.join(format!("zed-gui-{name}.stderr.log"));
        let before = zed_smoke::matching_processes(server)?;
        let mut child = zed_smoke::launch(
            zed,
            profile,
            workspace,
            &[file.to_path_buf()],
            &stdout,
            &stderr,
        )?;

        let server_pid = match zed_smoke::wait_for_server(
            &mut child,
            server,
            &before,
            profile,
            timeout,
        ) {
            Ok(pid) => pid,
            Err(error) => {
                let _ = zed_smoke::stop_zed(&mut child);
                return Err(error);
            }
        };

        thread::sleep(settle);
        let action_result = input_device().and_then(|mut input| action(&mut input));
        let stop_result = zed_smoke::stop_zed(&mut child);
        let server_stop_result =
            zed_smoke::ensure_server_stopped(server_pid, server, Duration::from_secs(3));
        let log_result = zed_smoke::scan_logs(profile, &stdout, &stderr);

        if let Err(error) = action_result {
            return Err(format!(
                "{name} GUI input failed: {error}{}",
                input_permission_hint()
            ));
        }
        stop_result?;
        server_stop_result?;
        log_result?;

        Ok(SessionEvidence {
            server_pid,
            stdout,
            stderr,
        })
    }

    fn input_device() -> Result<Enigo, String> {
        let mut settings = Settings::default();
        settings.open_prompt_to_get_permissions = false;
        let mut input = Enigo::new(&settings).map_err(|error| {
            format!(
                "initialize cross-platform input injection: {error}{}",
                input_permission_hint()
            )
        })?;
        input.set_delay(20);
        Ok(input)
    }

    fn press(input: &mut Enigo, key: Key, settle: Duration) -> Result<(), String> {
        input
            .key(key, Direction::Click)
            .map_err(|error| format!("press {key:?}: {error}"))?;
        thread::sleep(settle);
        Ok(())
    }

    fn type_text(input: &mut Enigo, text: &str, settle: Duration) -> Result<(), String> {
        input
            .text(text)
            .map_err(|error| format!("type {text:?}: {error}"))?;
        thread::sleep(settle);
        Ok(())
    }

    fn exercise_snippet(input: &mut Enigo, settle: Duration) -> Result<(), String> {
        press(input, Key::End, settle)?;
        press(input, Key::F14, settle)?;
        press(input, Key::F15, settle)?;
        type_text(input, "gui", settle)?;
        press(input, Key::F16, settle)?;
        type_text(input, "snippet", settle)?;
        press(input, Key::F16, settle)?;
        type_text(input, "1.2.3", settle)?;
        press(input, Key::F16, settle)?;
        press(input, Key::F17, settle)?;
        type_text(input, "2.0.0", settle)?;
        press(input, Key::F16, settle)?;
        type_text(input, "\n// GUI_SNIPPET_FINAL", settle)?;
        press(input, Key::F18, settle.saturating_mul(2))?;
        Ok(())
    }

    fn exercise_outline(input: &mut Enigo, settle: Duration) -> Result<(), String> {
        for (symbol, marker) in [
            ("alpha", "GUI_OUTLINE_ALPHA"),
            ("beta", "GUI_OUTLINE_BETA"),
            ("gamma", "GUI_OUTLINE_GAMMA"),
        ] {
            press(input, Key::F13, settle)?;
            type_text(input, symbol, settle)?;
            press(input, Key::Return, settle)?;
            press(input, Key::End, settle)?;
            type_text(input, &format!(" // {marker}"), settle)?;
            press(input, Key::F18, settle)?;
        }
        Ok(())
    }

    fn verify_snippet(path: &Path) -> Result<(), String> {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let expected = "package gui:snippet@2.0.0;";
        if !source.contains(expected) {
            return Err(format!(
                "snippet GUI qualification did not produce {expected:?}: {source:?}"
            ));
        }
        if !source.contains("// GUI_SNIPPET_FINAL") {
            return Err(format!("snippet final tab stop was not reached: {source:?}"));
        }
        for unexpected in ["wit-package", "example", "1.2.3"] {
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
        });
        fs::write(
            config.join("settings.json"),
            serde_json::to_vec_pretty(&settings)
                .map_err(|error| format!("encode isolated GUI settings: {error}"))?,
        )
        .map_err(|error| format!("write isolated GUI settings: {error}"))?;

        let keymap = json!([
            {
                "context": "Editor",
                "bindings": {
                    "f13": "outline::Toggle",
                    "f14": "editor::ShowCompletions",
                    "f18": "workspace::Save",
                }
            },
            {
                "context": "Editor && showing_completions",
                "bindings": {
                    "f15": "editor::ConfirmCompletion",
                }
            },
            {
                "context": "Editor && in_snippet && has_next_tabstop && !showing_completions",
                "bindings": {
                    "f16": "editor::NextSnippetTabstop",
                }
            },
            {
                "context": "Editor && in_snippet && has_previous_tabstop && !showing_completions",
                "bindings": {
                    "f17": "editor::PreviousSnippetTabstop",
                }
            }
        ]);
        fs::write(
            config.join("keymap.json"),
            serde_json::to_vec_pretty(&keymap)
                .map_err(|error| format!("encode isolated GUI keymap: {error}"))?,
        )
        .map_err(|error| format!("write isolated GUI keymap: {error}"))?;
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
            "; ensure the desktop session exposes a usable X11, Wayland virtual-keyboard, or libei input-injection backend"
        } else {
            ""
        }
    }
}

pub fn run(
    zed: &str,
    profile: &Path,
    timeout: Duration,
    settle: Duration,
    allow_input: bool,
) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        supported::run(zed, profile, timeout, settle, allow_input)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (zed, profile, timeout, settle, allow_input);
        Err("test-zed-gui supports Zed desktop hosts: macOS, Linux, and Windows".into())
    }
}
