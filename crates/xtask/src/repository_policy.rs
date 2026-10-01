use crate::util;
use std::{
    fs,
    path::{Component, Path},
};

const SKIP_DIRECTORIES: [&str; 6] = [".git", "target", "dist", "grammars", ".codex", ".omo"];

fn is_python_path(path: &Path) -> bool {
    if path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .any(|component| component == "__pycache__")
    {
        return true;
    }

    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "py" | "pyc" | "pyo" | "pyd"
            )
        })
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
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if path != root && SKIP_DIRECTORIES.contains(&name) {
            return Ok(());
        }
        if name == "__pycache__" {
            violations.push(
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string(),
            );
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
    }

    let workflows = root.join(".github/workflows");
    if path.starts_with(&workflows) {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("read workflow {}: {error}", path.display()))?;
        if text.to_ascii_lowercase().contains("python") {
            violations.push(format!(
                "{} (contains Python invocation/reference)",
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
            ));
        }
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
        println!("no Python source, cache artifacts, or workflow invocations found");
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
    fn recognizes_python_artifacts() {
        assert!(is_python_path(Path::new("scripts/check.py")));
        assert!(is_python_path(Path::new("scripts/cache.pyc")));
        assert!(is_python_path(Path::new("scripts/__pycache__/cache.bin")));
        assert!(!is_python_path(Path::new("crates/xtask/src/main.rs")));
        assert!(!is_python_path(Path::new("docs/python-policy.md")));
    }
}
