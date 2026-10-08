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

pub(crate) fn dev(args: &[String]) -> Result<(), String> {
    no_args(args, "dev")?;
    ensure_development_target(&util::repo_root(), &util::cargo_target_dir())?;
    let template = util::read_nonempty(&util::repo_root().join(".zed/settings.example.json"))?;
    if !template.contains("target/release/wit-language-server") {
        return Err(
            "example Zed settings no longer point to the local release language server".into(),
        );
    }
    build(&[
        "--kind".into(),
        "server".into(),
        "--release".into(),
        "true".into(),
    ])?;
    println!("Use .zed/settings.example.json as a template for untracked .zed/settings.json.");
    println!("No user or project settings were modified.");
    Ok(())
}

pub(crate) fn install_dev(args: &[String]) -> Result<(), String> {
    let mut options = opts(args, &["destination"])?;
    let destination =
        util::root_relative(util::required_option(&mut options, "destination")?.into());
    finish(options)?;
    let source = native_binary(None, true)?;
    copy_new_file(&source, &destination)
}

#[cfg(test)]
mod tests {
    use super::ensure_development_target;
    use std::path::Path;

    #[test]
    fn dev_rejects_a_build_path_different_from_committed_zed_settings() {
        let root = Path::new("example-repo");
        assert!(ensure_development_target(root, &root.join("target")).is_ok());
        assert!(ensure_development_target(root, &root.join("custom-target")).is_err());
    }
}
