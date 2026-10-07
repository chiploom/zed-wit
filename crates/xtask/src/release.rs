use crate::{dependency_policy::TARGETS, util};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet, env, fs, fs::OpenOptions, io::Write, path::Path, process::Command,
};

const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;

fn read_toml(path: &Path) -> Result<toml::Value, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn project_versions() -> Result<BTreeSet<String>, String> {
    let root = util::repo_root();
    let root_manifest = read_toml(&root.join("Cargo.toml"))?;
    let server_manifest = read_toml(&root.join("crates/wit-language-server/Cargo.toml"))?;
    let extension_manifest = read_toml(&root.join("extension.toml"))?;

    let root_version = root_manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "Cargo.toml omitted package.version".to_owned())?;
    let server_version = server_manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "language-server manifest omitted package.version".to_owned())?;
    let extension_version = extension_manifest
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml omitted version".to_owned())?;

    Ok([root_version, server_version, extension_version]
        .into_iter()
        .map(str::to_owned)
        .collect())
}

fn project_version() -> Result<String, String> {
    let versions = project_versions()?;
    if versions.len() != 1 {
        return Err("extension, adapter and native server versions differ".into());
    }
    versions
        .into_iter()
        .next()
        .ok_or_else(|| "project version set is empty".to_owned())
}

fn stable_tag_version(tag: &str) -> Option<&str> {
    let version = tag.strip_prefix('v')?;
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    if parts.iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (*part == "0" || !part.starts_with('0'))
    }) {
        Some(version)
    } else {
        None
    }
}

fn release_version_for_tag<'a>(tag: &'a str, project_version: &str) -> Result<&'a str, String> {
    let version = stable_tag_version(tag).ok_or("expected stable SemVer tag vX.Y.Z")?;
    if project_version != version {
        return Err("tag and adapter/server/extension versions must agree".into());
    }
    Ok(version)
}

fn git_revision(root: &Path) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("run git rev-parse: {error}"))?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map(|value| Some(value.trim().to_owned()))
            .map_err(|error| format!("git returned non-UTF-8 revision: {error}"))
    } else {
        Ok(None)
    }
}

fn asset_name(target: &str) -> String {
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("wit-language-server-{target}{suffix}")
}

fn ensure_regular_nonempty(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("stat {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("invalid release artifact: {}", path.display()));
    }
    Ok(())
}

fn json_string<'a>(value: &'a Value, key: &str, path: &Path) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} omitted string field {key:?}", path.display()))
}

pub fn package_release(target: &str, output: &Path) -> Result<(), String> {
    let root = util::repo_root();
    let name = asset_name(target);
    let source =
        root.join("target")
            .join(target)
            .join("release")
            .join(if target.contains("windows") {
                "wit-language-server.exe"
            } else {
                "wit-language-server"
            });
    ensure_regular_nonempty(&source)?;
    let metadata =
        fs::metadata(&source).map_err(|error| format!("stat {}: {error}", source.display()))?;
    if metadata.len() > MAX_BINARY_BYTES {
        return Err("release exceeds the adapter's 128 MiB binary size limit".into());
    }

    let version = project_version()?;
    fs::create_dir_all(output).map_err(|error| format!("create {}: {error}", output.display()))?;
    let artifact = output.join(&name);
    let checksum = output.join(format!("{name}.sha256"));
    let provenance = output.join(format!("{name}.provenance.json"));
    if [&artifact, &checksum, &provenance]
        .iter()
        .any(|path| path.exists())
    {
        return Err(format!(
            "refusing to overwrite release artifacts for {target}"
        ));
    }

    let revision = git_revision(&root)?;
    let dirty = !util::command_output("git", ["status", "--porcelain"], &root)?.is_empty();
    if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") && (dirty || revision.is_none()) {
        return Err("release CI checkout must be clean and committed".into());
    }

    fs::copy(&source, &artifact).map_err(|error| {
        format!(
            "copy {} to {}: {error}",
            source.display(),
            artifact.display()
        )
    })?;
    let digest = util::sha256_file(&artifact)?;
    util::write_new(&checksum, &format!("{digest}  {name}\n"))?;

    let rustc = util::command_output("rustc", ["--version", "--verbose"], &root)?;
    let provenance_value = json!({
        "artifact": name,
        "sha256": digest,
        "version": version,
        "target": target,
        "source_revision": revision,
        "source_dirty": dirty,
        "cargo_lock_sha256": util::sha256_file(&root.join("Cargo.lock"))?,
        "rustc": rustc,
        "build_command": format!(
            "cargo build -p wit-language-server --release --locked --target {target}"
        ),
        "workflow_run": env::var("GITHUB_RUN_ID").ok(),
    });
    let provenance_text = serde_json::to_string_pretty(&provenance_value)
        .map_err(|error| format!("serialize provenance: {error}"))?
        + "\n";
    util::write_new(&provenance, &provenance_text)?;

    println!("{}", artifact.display());
    Ok(())
}

pub fn validate_release(tag: &str) -> Result<(), String> {
    let root = util::repo_root();
    let project_version = project_version()?;
    let version = release_version_for_tag(tag, &project_version)?;

    let head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
    if !util::command_output("git", ["status", "--porcelain"], &root)?.is_empty() {
        return Err("release candidate checkout must be clean".into());
    }
    if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        let github_sha = env::var("GITHUB_SHA")
            .map_err(|_| "GITHUB_ACTIONS is true but GITHUB_SHA is missing")?;
        if github_sha != head {
            return Err("GITHUB_SHA does not match the validated release commit".into());
        }
    }

    if let Ok(output_path) = env::var("GITHUB_OUTPUT") {
        let mut output = OpenOptions::new()
            .append(true)
            .open(&output_path)
            .map_err(|error| format!("open GITHUB_OUTPUT {output_path}: {error}"))?;
        writeln!(output, "sha={head}\ntag={tag}")
            .map_err(|error| format!("write GITHUB_OUTPUT {output_path}: {error}"))?;
    } else if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        return Err("GITHUB_ACTIONS is true but GITHUB_OUTPUT is missing".into());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "tag": tag,
            "version": version,
            "sha": head,
            "result": "passed",
        }))
        .map_err(|error| format!("serialize release validation: {error}"))?
    );
    Ok(())
}

fn verify_license_notices(path: &Path, target: &str, lock_digest: &str) -> Result<(), String> {
    ensure_regular_nonempty(path)?;
    let text =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let expected_prefix = format!(
        "WIT language server redistribution notices\nTarget: {target}\nCargo.lock SHA256: {lock_digest}\n"
    );
    if !text.starts_with(&expected_prefix) {
        return Err(format!(
            "{} does not match target or Cargo.lock",
            path.display()
        ));
    }
    for required in [
        "Scope: target-filtered native normal/build dependency closure; dev-only edges excluded.",
        "\nPROJECT AND DIRECTLY COPIED MATERIAL\n",
        "\nPACKAGE: ",
        "\nRUST STANDARD LIBRARY: full notices from the pinned toolchain distribution\n",
    ] {
        if !text.contains(required) {
            return Err(format!(
                "{} is missing required redistribution section {required:?}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn verify_provenance(
    path: &Path,
    asset: &str,
    target: &str,
    digest: &str,
    version: &str,
    lock_digest: &str,
    revision: Option<&str>,
) -> Result<(), String> {
    ensure_regular_nonempty(path)?;
    let text =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;

    for (key, expected) in [
        ("artifact", asset),
        ("sha256", digest),
        ("version", version),
        ("target", target),
        ("cargo_lock_sha256", lock_digest),
    ] {
        let actual = json_string(&value, key, path)?;
        if actual != expected {
            return Err(format!(
                "{} has {key}={actual:?}, expected {expected:?}",
                path.display()
            ));
        }
    }

    let expected_command =
        format!("cargo build -p wit-language-server --release --locked --target {target}");
    if json_string(&value, "build_command", path)? != expected_command {
        return Err(format!("invalid build command in {}", path.display()));
    }

    if value.get("source_dirty").and_then(Value::as_bool) != Some(false) {
        return Err(format!(
            "{} was produced from a dirty checkout",
            path.display()
        ));
    }
    if json_string(&value, "rustc", path)?.trim().is_empty() {
        return Err(format!("{} has empty rustc provenance", path.display()));
    }
    if let Some(revision) = revision
        && json_string(&value, "source_revision", path)? != revision
    {
        return Err(format!(
            "{} source revision does not match the checked-out commit",
            path.display()
        ));
    }
    if let Ok(run_id) = env::var("GITHUB_RUN_ID")
        && value.get("workflow_run").and_then(Value::as_str) != Some(run_id.as_str())
    {
        return Err(format!(
            "{} workflow run does not match the verifier",
            path.display()
        ));
    }
    Ok(())
}

pub fn verify_release_assets(input: &Path) -> Result<(), String> {
    let root = util::repo_root();
    let version = project_version()?;
    let lock_digest = util::sha256_file(&root.join("Cargo.lock"))?;
    let revision = git_revision(&root)?;
    let mut expected = BTreeSet::new();

    for target in TARGETS {
        let name = asset_name(target);
        expected.insert(name.clone());
        expected.insert(format!("{name}.sha256"));
        expected.insert(format!("{name}.provenance.json"));
        expected.insert(format!("{name}.licenses.txt"));

        let asset = input.join(&name);
        ensure_regular_nonempty(&asset)?;
        let metadata =
            fs::metadata(&asset).map_err(|error| format!("stat {}: {error}", asset.display()))?;
        if metadata.len() > MAX_BINARY_BYTES {
            return Err(format!(
                "{} exceeds the 128 MiB release size limit",
                asset.display()
            ));
        }
        let digest = util::sha256_file(&asset)?;

        let sidecar = input.join(format!("{name}.sha256"));
        ensure_regular_nonempty(&sidecar)?;
        let expected_sidecar = format!("{digest}  {name}\n");
        let actual_sidecar = fs::read_to_string(&sidecar)
            .map_err(|error| format!("read {}: {error}", sidecar.display()))?;
        if actual_sidecar != expected_sidecar {
            return Err(format!("invalid checksum: {name}"));
        }

        verify_license_notices(
            &input.join(format!("{name}.licenses.txt")),
            target,
            &lock_digest,
        )?;

        verify_provenance(
            &input.join(format!("{name}.provenance.json")),
            &name,
            target,
            &digest,
            &version,
            &lock_digest,
            revision.as_deref(),
        )?;
    }

    let mut actual = BTreeSet::new();
    for entry in
        fs::read_dir(input).map_err(|error| format!("read {}: {error}", input.display()))?
    {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", input.display()))?;
        let path = entry.path();
        ensure_regular_nonempty(&path)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("non-UTF-8 release artifact: {}", path.display()))?;
        actual.insert(name);
    }
    if actual != expected {
        return Err(format!(
            "release artifact set does not match five complete targets\nexpected: {expected:?}\nactual: {actual:?}"
        ));
    }
    println!("verified {} release artifacts", actual.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_tag_validation() {
        assert_eq!(stable_tag_version("v0.1.0"), Some("0.1.0"));
        assert_eq!(stable_tag_version("v10.20.30"), Some("10.20.30"));
        assert_eq!(stable_tag_version("0.1.0"), None);
        assert_eq!(stable_tag_version("v01.1.0"), None);
        assert_eq!(stable_tag_version("v1.0.0-rc.1"), None);
        assert_eq!(stable_tag_version("v1.0"), None);
    }

    #[test]
    fn release_tag_must_match_project_version() {
        assert_eq!(
            release_version_for_tag("v0.1.0", "0.1.0").unwrap(),
            "0.1.0"
        );
        assert!(
            release_version_for_tag("v0.1.1", "0.1.0")
                .unwrap_err()
                .contains("versions must agree")
        );
        assert!(
            release_version_for_tag("0.1.0", "0.1.0")
                .unwrap_err()
                .contains("stable SemVer")
        );
    }

    #[test]
    fn release_asset_names_match_distribution_contract() {
        assert_eq!(
            asset_name("aarch64-apple-darwin"),
            "wit-language-server-aarch64-apple-darwin"
        );
        assert_eq!(
            asset_name("x86_64-pc-windows-msvc"),
            "wit-language-server-x86_64-pc-windows-msvc.exe"
        );
    }
}
