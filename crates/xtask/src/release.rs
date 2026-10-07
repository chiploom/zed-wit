use crate::{dependency_policy::TARGETS, util};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet, env, fs, fs::OpenOptions, io::Write, path::Path, process::Command,
};

const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseScope {
    Lsp,
    Extension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseValidationMode {
    New,
    Regenerate,
}

impl ReleaseValidationMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "new" => Ok(Self::New),
            "regenerate" => Ok(Self::Regenerate),
            _ => Err(format!(
                "unsupported release validation mode {value:?}; expected new or regenerate"
            )),
        }
    }
}

impl ReleaseScope {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "lsp" => Ok(Self::Lsp),
            "extension" => Ok(Self::Extension),
            _ => Err(format!(
                "unsupported release scope {value:?}; expected lsp or extension"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Lsp => "lsp",
            Self::Extension => "extension",
        }
    }

    fn title(self, version: &str) -> String {
        match self {
            Self::Lsp => format!("WIT Language Server v{version}"),
            Self::Extension => format!("WIT for Zed Extension v{version}"),
        }
    }

    fn notes_path(self, version: &str) -> String {
        format!("docs/releases/{}/v{version}.md", self.as_str())
    }
}

fn read_toml(path: &Path) -> Result<toml::Value, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn package_version(manifest: &toml::Value, label: &str) -> Result<String, String> {
    manifest
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} omitted package.version"))
}

fn extension_version_from(
    adapter_manifest: &toml::Value,
    extension_manifest: &toml::Value,
) -> Result<String, String> {
    let adapter_version = package_version(adapter_manifest, "Cargo.toml")?;
    let manifest_version = extension_manifest
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml omitted version".to_owned())?;

    if adapter_version != manifest_version {
        return Err("adapter crate and extension.toml versions differ".into());
    }
    Ok(adapter_version)
}

fn extension_version() -> Result<String, String> {
    let root = util::repo_root();
    extension_version_from(
        &read_toml(&root.join("Cargo.toml"))?,
        &read_toml(&root.join("extension.toml"))?,
    )
}

fn server_version_from(manifest: &toml::Value) -> Result<String, String> {
    package_version(manifest, "language-server manifest")
}

fn server_version_at(root: &Path) -> Result<String, String> {
    server_version_from(&read_toml(
        &root.join("crates/wit-language-server/Cargo.toml"),
    )?)
}

fn server_version() -> Result<String, String> {
    server_version_at(&util::repo_root())
}

fn runtime_lsp_version_from(manifest: &toml::Value) -> Result<String, String> {
    let version = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("zed-wit"))
        .and_then(|metadata| metadata.get("runtime-lsp-version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            "Cargo.toml omitted package.metadata.zed-wit.runtime-lsp-version".to_owned()
        })?;

    if stable_version(version).is_none() {
        return Err(format!(
            "runtime LSP pin {version:?} must be stable SemVer X.Y.Z"
        ));
    }
    Ok(version.to_owned())
}

pub(crate) fn runtime_lsp_version() -> Result<String, String> {
    let root = util::repo_root();
    runtime_lsp_version_from(&read_toml(&root.join("Cargo.toml"))?)
}

fn read_toml_at(root: &Path, revision: &str, path: &str) -> Result<toml::Value, String> {
    let spec = format!("{revision}:{path}");
    let output = Command::new("git")
        .args(["show", &spec])
        .current_dir(root)
        .output()
        .map_err(|error| format!("run git show {spec}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git show {spec} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let source = String::from_utf8(output.stdout)
        .map_err(|error| format!("git show {spec} returned non-UTF-8 data: {error}"))?;
    toml::from_str(&source).map_err(|error| format!("parse {spec}: {error}"))
}

fn stable_version(version: &str) -> Option<&str> {
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

fn tag_version_for_scope(scope: ReleaseScope, tag: &str) -> Result<&str, String> {
    let (prefix, expected_tag) = match scope {
        ReleaseScope::Lsp => ("v", "vX.Y.Z"),
        ReleaseScope::Extension => ("v-extension-", "v-extension-X.Y.Z"),
    };
    tag.strip_prefix(prefix)
        .and_then(stable_version)
        .ok_or_else(|| {
            format!(
                "expected stable {expected_tag} tag for {} release",
                scope.as_str()
            )
        })
}

fn release_version_for_scope<'a>(
    scope: ReleaseScope,
    tag: &'a str,
    expected_version: &str,
) -> Result<&'a str, String> {
    let version = tag_version_for_scope(scope, tag)?;
    if version != expected_version {
        return Err(format!(
            "{} release tag version {version} does not match expected version {expected_version}",
            scope.as_str()
        ));
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

    let version = server_version()?;
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

pub fn validate_release(tag: &str, scope: &str, mode: &str) -> Result<(), String> {
    let root = util::repo_root();
    let scope = ReleaseScope::parse(scope)?;
    let mode = ReleaseValidationMode::parse(mode)?;

    // Validate the user-provided ref syntax before passing it to any Git command.
    tag_version_for_scope(scope, tag)?;

    let control_head = util::command_output("git", ["rev-parse", "HEAD"], &root)?;
    if !util::command_output("git", ["status", "--porcelain"], &root)?.is_empty() {
        return Err("release control checkout must be clean".into());
    }
    if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        let github_sha = env::var("GITHUB_SHA")
            .map_err(|_| "GITHUB_ACTIONS is true but GITHUB_SHA is missing")?;
        if github_sha != control_head {
            return Err("GITHUB_SHA does not match the release control checkout".into());
        }
    }

    let source_sha = match mode {
        ReleaseValidationMode::New => control_head.clone(),
        ReleaseValidationMode::Regenerate => {
            util::command_output("git", ["rev-parse", &format!("{tag}^{{commit}}")], &root)?
        }
    };

    let (version, server_tag, runtime_lsp_version) = match scope {
        ReleaseScope::Lsp => {
            let server_version = match mode {
                ReleaseValidationMode::New => server_version()?,
                ReleaseValidationMode::Regenerate => server_version_from(&read_toml_at(
                    &root,
                    &source_sha,
                    "crates/wit-language-server/Cargo.toml",
                )?)?,
            };
            let version = release_version_for_scope(scope, tag, &server_version)?.to_owned();
            (version, format!("v{server_version}"), None)
        }
        ReleaseScope::Extension => {
            let (extension_version, runtime_lsp_version) = match mode {
                ReleaseValidationMode::New => (extension_version()?, runtime_lsp_version()?),
                ReleaseValidationMode::Regenerate => {
                    let adapter_manifest = read_toml_at(&root, &source_sha, "Cargo.toml")?;
                    let extension_manifest = read_toml_at(&root, &source_sha, "extension.toml")?;
                    (
                        extension_version_from(&adapter_manifest, &extension_manifest)?,
                        runtime_lsp_version_from(&adapter_manifest)?,
                    )
                }
            };
            let version = release_version_for_scope(scope, tag, &extension_version)?.to_owned();
            (
                version,
                format!("v{runtime_lsp_version}"),
                Some(runtime_lsp_version),
            )
        }
    };

    let title = scope.title(&version);
    let notes = scope.notes_path(&version);

    if let Ok(output_path) = env::var("GITHUB_OUTPUT") {
        let mut output = OpenOptions::new()
            .append(true)
            .open(&output_path)
            .map_err(|error| format!("open GITHUB_OUTPUT {output_path}: {error}"))?;
        writeln!(
            output,
            "sha={source_sha}\ntag={tag}\nscope={}\nversion={version}\ntitle={title}\nnotes={notes}\nserver_tag={server_tag}\nmode={}",
            scope.as_str(),
            match mode {
                ReleaseValidationMode::New => "new",
                ReleaseValidationMode::Regenerate => "regenerate",
            }
        )
        .map_err(|error| format!("write GITHUB_OUTPUT {output_path}: {error}"))?;
    } else if env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        return Err("GITHUB_ACTIONS is true but GITHUB_OUTPUT is missing".into());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "tag": tag,
            "version": version,
            "scope": scope.as_str(),
            "title": title,
            "notes": notes,
            "server_tag": server_tag,
            "runtime_lsp_version": runtime_lsp_version,
            "sha": source_sha,
            "mode": match mode {
                ReleaseValidationMode::New => "new",
                ReleaseValidationMode::Regenerate => "regenerate",
            },
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

struct ProvenanceExpectation<'a> {
    version: &'a str,
    lock_digest: &'a str,
    revision: Option<&'a str>,
    workflow_run: Option<&'a str>,
}

fn verify_provenance(
    path: &Path,
    asset: &str,
    target: &str,
    digest: &str,
    expected: &ProvenanceExpectation<'_>,
) -> Result<(), String> {
    ensure_regular_nonempty(path)?;
    let text =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;

    for (key, expected) in [
        ("artifact", asset),
        ("sha256", digest),
        ("version", expected.version),
        ("target", target),
        ("cargo_lock_sha256", expected.lock_digest),
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
    if let Some(revision) = expected.revision
        && json_string(&value, "source_revision", path)? != revision
    {
        return Err(format!(
            "{} source revision does not match the checked-out commit",
            path.display()
        ));
    }
    if let Some(run_id) = expected.workflow_run
        && value.get("workflow_run").and_then(Value::as_str) != Some(run_id)
    {
        return Err(format!(
            "{} workflow run does not match expected source run {run_id}",
            path.display()
        ));
    }
    Ok(())
}

fn verify_release_assets_against(
    input: &Path,
    source_root: &Path,
    expected_workflow_run: Option<&str>,
) -> Result<(), String> {
    let version = server_version_at(source_root)?;
    let lock_digest = util::sha256_file(&source_root.join("Cargo.lock"))?;
    let revision = git_revision(source_root)?;
    let provenance = ProvenanceExpectation {
        version: &version,
        lock_digest: &lock_digest,
        revision: revision.as_deref(),
        workflow_run: expected_workflow_run,
    };
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
            &provenance,
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

pub fn verify_release_assets(input: &Path) -> Result<(), String> {
    let root = util::repo_root();
    let expected_workflow_run = env::var("GITHUB_RUN_ID").ok();
    verify_release_assets_against(input, &root, expected_workflow_run.as_deref())
}

pub fn verify_restored_release_assets(
    input: &Path,
    source_root: &Path,
    source_run_id: &str,
) -> Result<(), String> {
    if source_run_id.is_empty() || !source_run_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("source run ID must contain only decimal digits".into());
    }
    let revision = git_revision(source_root)?
        .ok_or_else(|| format!("{} is not a Git checkout", source_root.display()))?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{} has invalid Git revision {revision:?}",
            source_root.display()
        ));
    }
    verify_release_assets_against(input, source_root, Some(source_run_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_release_versions_are_strict() {
        assert_eq!(stable_version("0.1.0"), Some("0.1.0"));
        assert_eq!(stable_version("10.20.30"), Some("10.20.30"));
        assert_eq!(stable_version("01.1.0"), None);
        assert_eq!(stable_version("1.0.0-rc.1"), None);
        assert_eq!(stable_version("1.0"), None);
    }

    #[test]
    fn runtime_lsp_pin_is_explicit_and_stable() {
        let manifest: toml::Value = toml::from_str(
            r#"
[package]
name = "zed-wit"
version = "1.2.0"

[package.metadata.zed-wit]
runtime-lsp-version = "0.7.3"
"#,
        )
        .unwrap();
        assert_eq!(runtime_lsp_version_from(&manifest).unwrap(), "0.7.3");

        let invalid: toml::Value = toml::from_str(
            r#"
[package]
name = "zed-wit"
version = "1.2.0"

[package.metadata.zed-wit]
runtime-lsp-version = "0.7.3-rc.1"
"#,
        )
        .unwrap();
        assert!(runtime_lsp_version_from(&invalid).is_err());
    }

    #[test]
    fn release_validation_modes_are_explicit() {
        assert_eq!(
            ReleaseValidationMode::parse("new").unwrap(),
            ReleaseValidationMode::New
        );
        assert_eq!(
            ReleaseValidationMode::parse("regenerate").unwrap(),
            ReleaseValidationMode::Regenerate
        );
        assert!(ReleaseValidationMode::parse("restore").is_err());
    }

    #[test]
    fn scope_tag_syntax_is_validated_before_resolution() {
        assert_eq!(
            tag_version_for_scope(ReleaseScope::Lsp, "v0.1.0").unwrap(),
            "0.1.0"
        );
        assert_eq!(
            tag_version_for_scope(ReleaseScope::Extension, "v-extension-2.3.4").unwrap(),
            "2.3.4"
        );
        for invalid in ["--help", "main", "v1.0", "v1.0.0-rc.1", "v-extension-main"] {
            assert!(tag_version_for_scope(ReleaseScope::Lsp, invalid).is_err());
        }
    }

    #[test]
    fn scope_tags_match_their_independent_versions() {
        assert_eq!(
            release_version_for_scope(ReleaseScope::Lsp, "v0.2.0", "0.2.0").unwrap(),
            "0.2.0"
        );
        assert_eq!(
            release_version_for_scope(ReleaseScope::Extension, "v-extension-1.4.0", "1.4.0",)
                .unwrap(),
            "1.4.0"
        );
        assert!(
            release_version_for_scope(ReleaseScope::Lsp, "v0.2.1", "0.2.0")
                .unwrap_err()
                .contains("does not match")
        );
        assert!(
            release_version_for_scope(ReleaseScope::Extension, "v1.4.0", "1.4.0")
                .unwrap_err()
                .contains("v-extension-X.Y.Z")
        );
    }

    #[test]
    fn scope_version_validation_is_independent() {
        assert_eq!(
            release_version_for_scope(ReleaseScope::Lsp, "v2.0.0", "2.0.0").unwrap(),
            "2.0.0"
        );
        assert_eq!(
            release_version_for_scope(ReleaseScope::Extension, "v-extension-9.1.0", "9.1.0",)
                .unwrap(),
            "9.1.0"
        );
    }

    #[test]
    fn release_scope_controls_title_and_notes_path() {
        let lsp = ReleaseScope::parse("lsp").unwrap();
        assert_eq!(lsp.as_str(), "lsp");
        assert_eq!(lsp.title("0.2.0"), "WIT Language Server v0.2.0");
        assert_eq!(lsp.notes_path("0.2.0"), "docs/releases/lsp/v0.2.0.md");

        let extension = ReleaseScope::parse("extension").unwrap();
        assert_eq!(extension.as_str(), "extension");
        assert_eq!(extension.title("1.4.0"), "WIT for Zed Extension v1.4.0");
        assert_eq!(
            extension.notes_path("1.4.0"),
            "docs/releases/extension/v1.4.0.md"
        );

        assert!(ReleaseScope::parse("full").is_err());
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
