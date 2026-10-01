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
    if python_shebang || (should_scan_for_invocations(root, path) && contains_python_invocation(&text))
    {
        violations.push(format!(
            "{} (contains Python tooling or invocation)",
            path.strip_prefix(root).unwrap_or(path).display()
        ));
    }
    Ok(())
}

fn visit(root: &Path, path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("stat {}: {error}", path.display()))?;
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
        for entry in fs::read_dir(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?
        {
            let entry =
                entry.map_err(|error| format!("read {} entry: {error}", path.display()))?;
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
}
