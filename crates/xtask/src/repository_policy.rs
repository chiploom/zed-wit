use crate::util;
use std::{
    fs,
    path::{Component, Path},
};

const SKIP_DIRECTORIES: [&str; 6] = [".git", "target", "dist", "grammars", ".codex", ".omo"];
const PYTHON_DIRECTORIES: [&str; 3] = ["__pycache__", ".venv", "venv"];
const PYTHON_TOOLING_FILES: [&str; 7] = [
    "pyproject.toml",
    "pipfile",
    "pipfile.lock",
    "poetry.lock",
    "uv.lock",
    ".python-version",
    "tox.ini",
];

fn normalized_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_python_path(path: &Path) -> bool {
    if path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .any(|component| {
            PYTHON_DIRECTORIES
                .iter()
                .any(|candidate| component.eq_ignore_ascii_case(candidate))
        })
    {
        return true;
    }

    let name = normalized_name(path);
    if PYTHON_TOOLING_FILES.contains(&name.as_str())
        || (name.starts_with("requirements") && name.ends_with(".txt"))
    {
        return true;
    }

    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "py" | "pyi" | "pyc" | "pyo" | "pyd"
            )
        })
}

fn should_scan_for_invocations(root: &Path, path: &Path) -> bool {
    if path.starts_with(root.join(".github/workflows")) {
        return true;
    }
    let name = normalized_name(path);
    if matches!(
        name.as_str(),
        "makefile" | "justfile" | "taskfile.yml" | "taskfile.yaml"
    ) {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "sh" | "bash" | "zsh" | "fish" | "ps1"
            )
        })
}

fn contains_python_invocation(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "setup-python",
        "python3 ",
        "python3\n",
        "python -m ",
        "python ",
        "pytest",
        "pip3 ",
        "pip ",
        "poetry ",
        "uv run ",
        "uv sync",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn inspect_text_file(root: &Path, path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("stat {}: {error}", path.display()))?;
    if metadata.len() > 1024 * 1024 {
        return Ok(());
    }
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let first_line = text.lines().next().unwrap_or_default().to_ascii_lowercase();
    let python_shebang = first_line.starts_with("#!") && first_line.contains("python");
    if python_shebang
        || (should_scan_for_invocations(root, path) && contains_python_invocation(&text))
    {
        violations.push(format!(
            "{} (contains Python tooling or invocation)",
            path.strip_prefix(root).unwrap_or(path).display()
        ));
    }
    Ok(())
}

fn inspect_tracked_files(root: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let tracked = util::command_output("git", ["ls-files", "-z"], root)?;
    for relative in tracked.split('\0').filter(|path| !path.is_empty()) {
        let path = root.join(relative);
        if is_python_path(&path) {
            violations.push(relative.to_owned());
            continue;
        }

        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("stat tracked file {}: {error}", path.display()))?;
        if !metadata.file_type().is_symlink() {
            inspect_text_file(root, &path, violations)?;
        }
    }
    Ok(())
}

fn visit(root: &Path, path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("stat {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        if is_python_path(path) {
            violations.push(
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string(),
            );
        }
        return Ok(());
    }

    if metadata.is_dir() {
        let name = normalized_name(path);
        if path != root && PYTHON_DIRECTORIES.contains(&name.as_str()) {
            violations.push(
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string(),
            );
            return Ok(());
        }
        if path != root && SKIP_DIRECTORIES.contains(&name.as_str()) {
            return Ok(());
        }
        for entry in
            fs::read_dir(path).map_err(|error| format!("read {}: {error}", path.display()))?
        {
            let entry = entry.map_err(|error| format!("read {} entry: {error}", path.display()))?;
            visit(root, &entry.path(), violations)?;
        }
        return Ok(());
    }

    if is_python_path(path) {
        violations.push(
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string(),
        );
    } else {
        inspect_text_file(root, path, violations)?;
    }
    Ok(())
}

pub fn check_no_python() -> Result<(), String> {
    let root = util::repo_root();
    let mut violations = Vec::new();
    visit(&root, &root, &mut violations)?;
    inspect_tracked_files(&root, &mut violations)?;
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        println!("no Python source, tooling, cache artifacts, or invocations found");
        Ok(())
    } else {
        Err(format!(
            "Python is not permitted in this repository:\n{}",
            violations
                .iter()
                .map(|path| format!("  - {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_python_artifacts_and_tooling_files() {
        for path in [
            "scripts/check.py",
            "types/api.pyi",
            "scripts/cache.pyc",
            "scripts/__pycache__/cache.bin",
            ".venv/bin/tool",
            "pyproject.toml",
            "requirements-dev.txt",
            "uv.lock",
        ] {
            assert!(is_python_path(Path::new(path)), "{path}");
        }
        assert!(!is_python_path(Path::new("crates/xtask/src/main.rs")));
        assert!(!is_python_path(Path::new("docs/python-policy.md")));
    }

    #[test]
    fn recognizes_python_invocations() {
        assert!(contains_python_invocation("python3 scripts/check.py"));
        assert!(contains_python_invocation("uses: actions/setup-python@abc"));
        assert!(contains_python_invocation("uv run pytest"));
        assert!(!contains_python_invocation("cargo xtask check-no-python"));
    }

    #[test]
    fn protected_cd_orders_asset_digest_checks_around_promotion() {
        let root = util::repo_root();
        let workflow = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let steps = [
            "Capture exact local release asset digests",
            "Create or resume draft release",
            "Upload LSP release assets",
            "Verify draft asset scope",
            "Verify uploaded draft asset digests and state",
            "Require unchanged unpublished draft immediately before promotion",
            "Recheck immutable-release enforcement before promotion",
            "Reverify exact draft asset digests immediately before promotion",
            "Publish draft release",
            "Verify published release",
            "Verify immutable published asset digests",
        ];
        let mut positions = Vec::new();
        for name in steps {
            let marker = format!("      - name: {name}\n");
            let offset = workflow
                .find(&marker)
                .expect("missing required release integrity step");
            assert_eq!(workflow.match_indices(&marker).count(), 1);
            positions.push(offset);
        }
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let verifier = "bash release-source/scripts/release-asset-integrity.sh";
        for index in [0, 4, 7, 10] {
            let start = positions[index];
            let end = positions.get(index + 1).copied().unwrap_or(workflow.len());
            assert!(
                workflow[start..end].contains(verifier),
                "release integrity step {} does not invoke the shared production verifier",
                steps[index]
            );
        }
        assert!(workflow[positions[7]..positions[8]].contains(" draft"));
        assert!(workflow[positions[10]..].contains(" published"));
    }

    #[cfg(unix)]
    #[test]
    fn release_tag_ruleset_policy_accepts_only_the_audited_revision() {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let workflow =
            fs::read_to_string(util::repo_root().join(".github/workflows/release.yml")).unwrap();
        let marker = "          if ! jq -e '\n            . as $details\n";
        let start = workflow
            .find(marker)
            .expect("missing release tag policy filter")
            + "          if ! jq -e '\n".len();
        let end = workflow[start..]
            .find("' <<<\"$details\" >/dev/null; then")
            .map(|offset| start + offset)
            .expect("missing release tag policy filter terminator");
        let filter = &workflow[start..end];
        let valid = serde_json::json!([{
            "id": 24620024,
            "source_type": "Repository",
            "source": "chiploom/zed-wit",
            "target": "tag",
            "enforcement": "active",
            "updated_at": "2026-10-06T21:11:44.860-04:00",
            "conditions": {"ref_name": {"include": ["refs/tags/v*"], "exclude": []}},
            "rules": [
                {"type": "update"},
                {"type": "deletion"},
                {"type": "non_fast_forward"}
            ]
        }]);
        let passes = |fixture: &serde_json::Value| -> bool {
            let mut child = Command::new("jq")
                .arg("-e")
                .arg(filter)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("jq is required by protected CD and release policy testing");
            child
                .stdin
                .take()
                .unwrap()
                .write_all(fixture.to_string().as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.code() != Some(3),
                "invalid production jq policy: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.status.success()
        };

        assert!(
            passes(&valid),
            "redacted bypass field must use the audited pin"
        );
        let mut changed = valid.clone();
        changed[0]["bypass_actors"] = serde_json::json!([]);
        assert!(passes(&changed), "visible empty bypass list must pass");
        changed[0]["bypass_actors"] =
            serde_json::json!([{"actor_id": 1, "actor_type": "User", "bypass_mode": "always"}]);
        assert!(!passes(&changed), "a visible bypass actor must fail");
        changed = valid.clone();
        changed[0]["updated_at"] = serde_json::json!("2026-10-13T23:00:00Z");
        assert!(
            !passes(&changed),
            "any unreviewed ruleset revision must fail"
        );
        changed = valid.clone();
        changed[0]["id"] = serde_json::json!(24620025);
        assert!(!passes(&changed), "an unreviewed ruleset ID must fail");
        changed = valid.clone();
        changed[0]["conditions"]["ref_name"]["exclude"] = serde_json::json!(["refs/tags/v-bad"]);
        assert!(!passes(&changed));
        changed = valid.clone();
        changed[0]["rules"][2]["type"] = serde_json::json!("creation");
        assert!(!passes(&changed), "tag creation must remain possible");
        changed = valid.clone();
        changed[0]["rules"] = serde_json::json!([{"type":"update"}, {"type":"deletion"}]);
        assert!(!passes(&changed), "all three tag protections are required");
    }

    #[test]
    fn draft_asset_scope_uses_draft_capable_release_lookup() {
        let root = util::repo_root();
        let workflow = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let start = workflow
            .find("      - name: Verify draft asset scope\n")
            .unwrap();
        let end = workflow[start..]
            .find("\n      - name: Require unchanged unpublished draft immediately before promotion\n")
            .or_else(|| workflow[start..].find("\n      - name: Publish draft release\n"))
            .map(|offset| start + offset)
            .unwrap();
        let step = &workflow[start..end];

        assert!(
            step.contains(r#"gh release view "$RELEASE_TAG" --json isDraft,tagName,name,assets"#)
        );
        assert!(!step.contains(r#"releases/tags/$RELEASE_TAG"#));
        assert!(step.contains(r#".isDraft"#));
    }
}
