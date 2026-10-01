use crate::{dependency_policy, util};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

const NOTICE_PREFIXES: [&str; 5] = ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"];

fn package_string<'a>(package: &'a Value, key: &str) -> Result<&'a str, String> {
    package
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("cargo package omitted {key}: {package}"))
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|error| format!("read {}: {error}", path.display()))? {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", path.display()))?;
        let candidate = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("stat {}: {error}", candidate.display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "refusing to follow symlink while collecting notices: {}",
                candidate.display()
            ));
        }
        if file_type.is_dir() {
            collect_files(&candidate, files)?;
        } else if file_type.is_file() {
            files.push(candidate);
        }
    }
    Ok(())
}

fn package_notices(package: &Value) -> Result<Vec<(String, String)>, String> {
    let root = util::repo_root();
    let manifest_path = PathBuf::from(package_string(package, "manifest_path")?);
    let directory = manifest_path
        .parent()
        .ok_or_else(|| format!("manifest has no parent: {}", manifest_path.display()))?;
    let id = package_string(package, "id")?;

    if id.starts_with("path+") && directory.strip_prefix(&root).is_ok() {
        return ["LICENSE-MIT", "LICENSE-APACHE"]
            .into_iter()
            .map(|name| Ok((name.to_owned(), util::read_nonempty(&root.join(name))?)))
            .collect();
    }

    let mut candidates = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("read {}: {error}", directory.display()))?
    {
        let entry =
            entry.map_err(|error| format!("read {} entry: {error}", directory.display()))?;
        let name = entry.file_name().to_string_lossy().to_uppercase();
        if NOTICE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("stat {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "refusing to follow symlink while collecting notices: {}",
                    path.display()
                ));
            }
            if file_type.is_dir() {
                collect_files(&path, &mut candidates)?;
            } else if file_type.is_file() {
                candidates.push(path);
            }
        }
    }

    if let Some(license_file) = package.get("license_file").and_then(Value::as_str) {
        let explicit = directory.join(license_file);
        let canonical_directory = fs::canonicalize(directory)
            .map_err(|error| format!("canonicalize {}: {error}", directory.display()))?;
        let canonical_explicit = fs::canonicalize(&explicit)
            .map_err(|error| format!("canonicalize {}: {error}", explicit.display()))?;
        if !canonical_explicit.starts_with(&canonical_directory) {
            return Err(format!(
                "license path leaves package {}: {}",
                package_string(package, "name")?,
                explicit.display()
            ));
        }
        candidates.push(canonical_explicit);
    }

    candidates.sort();
    candidates.dedup();
    if !candidates.is_empty() {
        return candidates
            .into_iter()
            .map(|path| {
                let name = path
                    .strip_prefix(directory)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                Ok((name, util::read_nonempty(&path)?))
            })
            .collect();
    }

    let vcs_file = directory.join(".cargo_vcs_info.json");
    let vcs = if vcs_file.exists() {
        let value: Value = serde_json::from_str(&util::read_nonempty(&vcs_file)?)
            .map_err(|error| format!("parse {}: {error}", vcs_file.display()))?;
        value
            .get("git")
            .and_then(|git| git.get("sha1"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };

    let name = package_string(package, "name")?;
    let version = package_string(package, "version")?;
    match (name, version, vcs.as_deref()) {
        ("topiary-core", "0.7.3", Some(revision @ "75ce8324ebaef45e00a964f110ed18ca3ed80235")) => {
            Ok(vec![(
                format!("upstream root LICENSE at {revision}"),
                util::read_nonempty(&root.join(".github/licenses/topiary-core-0.7.3/LICENSE"))?,
            )])
        }
        ("backtrace-ext", "0.2.1", Some("043c95350875a36be6cd755dcef21a44a52ec2cc")) => Ok(vec![
            (
                "Cargo.toml.orig (upstream license declaration)".to_owned(),
                util::read_nonempty(&directory.join("Cargo.toml.orig"))?,
            ),
            (
                "README.md (upstream attribution)".to_owned(),
                util::read_nonempty(&directory.join("README.md"))?,
            ),
            (
                "Apache-2.0 canonical text; selected from declared MIT OR Apache-2.0".to_owned(),
                util::read_nonempty(&root.join("LICENSE-APACHE"))?,
            ),
        ]),
        _ => Err(format!(
            "no license files for {name} {version}; review upstream source"
        )),
    }
}

fn render(target: &str, data: &Value, sysroot: &Path) -> Result<(String, usize), String> {
    let root = util::repo_root();
    let packages = dependency_policy::check(data)?;
    let rust_docs = sysroot.join("share/doc/rust");
    let standard_library = util::read_nonempty(&rust_docs.join("COPYRIGHT-library.html"))?;
    let license_dir = rust_docs.join("licenses");
    if !license_dir.is_dir() {
        return Err("pinned Rust distribution is missing its license directory".into());
    }

    let mut sections = vec![
        "WIT language server redistribution notices".to_owned(),
        format!("Target: {target}"),
        format!(
            "Cargo.lock SHA256: {}",
            util::sha256_file(&root.join("Cargo.lock"))?
        ),
        "Scope: target-filtered native normal/build dependency closure; dev-only edges excluded."
            .to_owned(),
        "Build dependencies and workspace feature unification may conservatively include code not linked into the executable."
            .to_owned(),
        "License choices below preserve every AND obligation. Other offered alternatives are not required by these choices."
            .to_owned(),
        "Platform system libraries supplied by the operating system are not bundled by this project."
            .to_owned(),
        format!(
            "\nPROJECT AND DIRECTLY COPIED MATERIAL\n{}",
            util::read_nonempty(&root.join("THIRD_PARTY_NOTICES.md"))?
        ),
    ];

    for package in &packages {
        let name = package_string(package, "name")?;
        let version = package_string(package, "version")?;
        let source = package
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("workspace: https://github.com/chiploom/zed-wit");
        let repository = package
            .get("repository")
            .and_then(Value::as_str)
            .unwrap_or("not declared");
        let authors = package
            .get("authors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let declared_license = package
            .get("license")
            .and_then(Value::as_str)
            .unwrap_or("not declared");
        sections.extend([
            format!("\n{}", "=".repeat(78)),
            format!("PACKAGE: {name} {version}"),
            format!("Source: {source}"),
            format!("Repository: {repository}"),
            format!("Authors: {authors}"),
            format!("Declared license: {declared_license}"),
            format!(
                "Selected license: {}",
                dependency_policy::license_choice(package)?
            ),
        ]);
        for (notice_name, text) in package_notices(package)? {
            sections.extend([format!("\n--- {notice_name} ---\n"), text]);
        }
    }

    sections.extend([
        format!("\n{}", "=".repeat(78)),
        "RUST STANDARD LIBRARY: full notices from the pinned toolchain distribution".to_owned(),
        "The standard library offers MIT OR Apache-2.0; retain the embedded third-party notices and exceptions below."
            .to_owned(),
        format!(
            "COPYRIGHT-library.html (complete upstream document):\n{standard_library}"
        ),
    ]);

    let mut license_files = fs::read_dir(&license_dir)
        .map_err(|error| format!("read {}: {error}", license_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    license_files.sort();
    if license_files.is_empty() {
        return Err("pinned Rust distribution has no standard-library license texts".into());
    }
    for file in license_files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("non-UTF-8 Rust license path: {}", file.display()))?;
        sections.extend([
            format!("\n--- Rust distribution licenses/{name} ---\n"),
            util::read_nonempty(&file)?,
        ]);
    }

    Ok((sections.join("\n") + "\n", packages.len()))
}

pub fn run(target: &str, output: &Path) -> Result<(), String> {
    let root = util::repo_root();
    let sysroot = PathBuf::from(util::command_output(
        "rustc",
        ["--print", "sysroot"],
        &root,
    )?);
    let data = dependency_policy::metadata(Some(target))?;
    let (content, count) = render(target, &data, &sysroot)?;
    fs::create_dir_all(output).map_err(|error| format!("create {}: {error}", output.display()))?;
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = output.join(format!("wit-language-server-{target}{suffix}.licenses.txt"));
    util::write_new(&path, &content)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "target": target,
            "packages": count,
            "output": path.display().to_string(),
            "bytes": content.len(),
        }))
        .map_err(|error| format!("serialize license report: {error}"))?
    );
    Ok(())
}
