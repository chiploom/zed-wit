//! Native server and adapter builds, plus developer installs.
use super::common::{
    bool_option, cargo, copy_new_file, finish, host_target, native_binary, no_args, opts, run,
};
use crate::{dependency_policy, util};

pub(crate) fn release_build(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["target", "output"])?;
    let target = match options.remove("target") {
        Some(target) => target,
        None => host_target()?,
    };
    dependency_policy::ensure_target(&target)?;
    let output = options
        .remove("output")
        .map(|value| util::root_relative(value.into()));
    finish(options)?;
    cargo(&[
        "build",
        "-p",
        "wit-language-server",
        "--release",
        "--locked",
        "--target",
        &target,
    ])?;
    if let Some(destination) = output {
        copy_new_file(&native_binary(Some(&target), true)?, &destination)?;
    }
    Ok(())
}

pub(crate) fn build(args: &[String]) -> Result<(), String> {
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
            command.extend([
                "-p".into(),
                "zed-wit".into(),
                "--target".into(),
                "wasm32-wasip2".into(),
            ]);
        }
        _ => return Err(format!("unknown build kind {kind:?}")),
    }
    if optimized {
        command.push("--release".into());
    }
    run("cargo", &command)
}

fn ensure_development_target(
    root: &std::path::Path,
    actual: &std::path::Path,
) -> Result<(), String> {
    if actual != root.join("target") {
        return Err(
            "dev's committed Zed settings example requires the default target/ directory; customize Zed settings for a non-default CARGO_TARGET_DIR"
                .into(),
        );
    }
    Ok(())
}

fn development_binary_from_cargo_messages(messages: &str) -> Result<std::path::PathBuf, String> {
    let mut binaries = std::collections::BTreeSet::new();
    for line in messages.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(|v| v.as_str()) != Some("compiler-artifact")
            || value.pointer("/target/name").and_then(|v| v.as_str())
                != Some("wit-language-server")
        {
            continue;
        }
        if let Some(path) = value.get("executable").and_then(|v| v.as_str()) {
            binaries.insert(std::path::PathBuf::from(path));
        }
    }
    if binaries.len() != 1 {
        return Err(format!(
            "expected one native language server executable in Cargo build messages, found {}",
            binaries.len()
        ));
    }
    Ok(binaries.into_iter().next().expect("exactly one binary"))
}

pub(crate) fn dev(args: &[String]) -> Result<(), String> {
    no_args(args, "dev")?;
    ensure_development_target(&util::repo_root(), &util::cargo_target_dir())?;
    let template = util::read_nonempty(&util::repo_root().join(".zed/settings.example.json"))?;
    if !template.contains("target/release/wit-language-server") {
        return Err(
            "example Zed settings no longer point to the local release language server".into(),
        );
    }
    // Cargo's build.target configuration can implicitly select a target
    // triple and move the binary to target/<triple>/release. Read the actual
    // compiler artifact path; an old default-path executable is not proof.
    let output = std::process::Command::new("cargo")
        .args([
            "build",
            "-p",
            "wit-language-server",
            "--release",
            "--locked",
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(util::repo_root())
        .output()
        .map_err(|error| format!("run Cargo development build: {error}"))?;
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("Cargo development build failed with {}", output.status));
    }
    let messages = String::from_utf8(output.stdout)
        .map_err(|error| format!("Cargo development build output was not UTF-8: {error}"))?;
    let actual = development_binary_from_cargo_messages(&messages)?;
    let expected = util::repo_root().join("target").join("release").join(
        if cfg!(windows) {
            "wit-language-server.exe"
        } else {
            "wit-language-server"
        },
    );
    if actual != expected {
        return Err(format!(
            "Cargo placed the development server at {}, but committed Zed settings require {}; remove the implicit build.target configuration or configure Zed manually",
            actual.display(),
            expected.display()
        ));
    }
    println!("Use .zed/settings.example.json as a template for untracked .zed/settings.json.");
    println!("No user or project settings were modified.");
    Ok(())
}

pub(crate) fn install_dev(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["destination", "target"])?;
    let destination =
        util::root_relative(util::required_option(&mut options, "destination")?.into());
    let target = match options.remove("target") {
        Some(target) => target,
        None => host_target()?,
    };
    dependency_policy::ensure_target(&target)?;
    finish(options)?;
    // Fail before invoking Cargo when the requested install path is occupied.
    // The final create_new operation remains the authoritative race guard.
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => return Err(format!("destination already exists: {}", destination.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("stat {}: {error}", destination.display())),
    }
    // Rebuild to avoid copying a stale binary, always using Cargo's explicit
    // target-specific layout. This matches release-build on every host.
    release_build(&["--target".into(), target.clone()])?;
    copy_new_file(&native_binary(Some(&target), true)?, &destination)
}

#[cfg(test)]
mod tests {
    use super::ensure_development_target;
    use std::path::Path;

    #[test]
    fn dev_tracks_the_real_cargo_executable_instead_of_a_stale_default_path() {
        let artifact = serde_json::json!({
            "reason": "compiler-artifact",
            "target": {"name": "wit-language-server"},
            "executable": "/tmp/alternate-target/aarch64-apple-darwin/release/wit-language-server"
        });
        let messages = format!("{}\n", artifact);
        let actual = super::development_binary_from_cargo_messages(&messages).unwrap();
        assert_eq!(
            actual,
            std::path::PathBuf::from(
                "/tmp/alternate-target/aarch64-apple-darwin/release/wit-language-server"
            )
        );
        assert!(super::development_binary_from_cargo_messages("").is_err());
    }

    #[test]
    fn implicit_configured_target_changes_the_real_cargo_artifact_path() {
        use std::{fs, process::Command, sync::atomic::{AtomicU64, Ordering}};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let candidate = std::env::temp_dir().join(format!(
                "zed-wit-implicit-target-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create Cargo fixture: {error}"),
            }
        };
        let root = fs::canonicalize(root).expect("canonicalize Cargo fixture root");
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).expect("remove Cargo fixture");
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join(".cargo")).unwrap();
        fs::create_dir(root.join("cargo-home")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"wit-language-server\"\nversion = \"0.0.1\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let host = super::host_target().unwrap();
        fs::write(
            root.join(".cargo/config.toml"),
            format!("[build]\ntarget = \"{host}\"\n"),
        )
        .unwrap();
        let output = Command::new("cargo")
            .args([
                "build",
                "--release",
                "--offline",
                "--message-format=json-render-diagnostics",
            ])
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env("CARGO_HOME", root.join("cargo-home"))
            .current_dir(&root)
            .output()
            .expect("build Cargo test fixture");
        assert!(
            output.status.success(),
            "fixture compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = super::development_binary_from_cargo_messages(
            &String::from_utf8(output.stdout).unwrap(),
        )
        .unwrap();
        let binary_name = if cfg!(windows) {
            "wit-language-server.exe"
        } else {
            "wit-language-server"
        };
        assert_eq!(actual, root.join("target").join(&host).join("release").join(binary_name));
        assert_ne!(actual, root.join("target").join("release").join(binary_name));
    }

    #[test]
    fn dev_rejects_a_build_path_different_from_committed_zed_settings() {
        let root = Path::new("example-repo");
        assert!(ensure_development_target(root, &root.join("target")).is_ok());
        assert!(ensure_development_target(root, &root.join("custom-target")).is_err());
    }
}
