use crate::util;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};

pub const TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

const GRAMMAR_REPOSITORY: &str = "https://github.com/bytecodealliance/tree-sitter-wit";
const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn grammar_revision(syntax_manifest: &str, extension_manifest: &str) -> Result<String, String> {
    let syntax: toml::Value = toml::from_str(syntax_manifest)
        .map_err(|error| format!("parse wit-syntax Cargo.toml: {error}"))?;
    let extension: toml::Value = toml::from_str(extension_manifest)
        .map_err(|error| format!("parse extension.toml: {error}"))?;

    let grammar = extension
        .get("grammars")
        .and_then(|grammars| grammars.get("wit"))
        .ok_or_else(|| "extension.toml has no grammars.wit table".to_owned())?;
    let extension_repo = grammar
        .get("repository")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml has no grammars.wit.repository".to_owned())?;
    let extension_rev = grammar
        .get("rev")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "extension.toml has no grammars.wit.rev".to_owned())?;

    let syntax_grammar = syntax
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("tree-sitter-wit"))
        .ok_or_else(|| "wit-syntax Cargo.toml has no tree-sitter-wit dependency".to_owned())?;
    let syntax_repo = syntax_grammar
        .get("git")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "wit-syntax Cargo.toml has no tree-sitter-wit.git".to_owned())?;
    let syntax_rev = syntax_grammar
        .get("rev")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "wit-syntax Cargo.toml has no tree-sitter-wit.rev".to_owned())?;

    if extension_repo != GRAMMAR_REPOSITORY || syntax_repo != GRAMMAR_REPOSITORY {
        return Err(format!(
            "unreviewed tree-sitter-wit repository: extension={extension_repo:?}, syntax={syntax_repo:?}"
        ));
    }
    if extension_rev != syntax_rev {
        return Err(format!(
            "grammar pin mismatch: extension={extension_rev}, syntax={syntax_rev}"
        ));
    }
    if syntax_rev.len() != 40
        || !syntax_rev
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "invalid tree-sitter-wit commit revision: {syntax_rev:?}"
        ));
    }

    Ok(syntax_rev.to_owned())
}

fn pinned_grammar_revision() -> Result<String, String> {
    let root = util::repo_root();
    grammar_revision(
        &util::read_nonempty(&root.join("crates/wit-syntax/Cargo.toml"))?,
        &util::read_nonempty(&root.join("extension.toml"))?,
    )
}

fn single_version(packages: &[Value], name: &str, scope: &str) -> Result<String, String> {
    let matches = packages
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some(name))
        .map(|package| {
            Ok((
                package_string(package, "id")?,
                package_string(package, "version")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if matches.len() != 1 {
        return Err(format!(
            "{scope}: expected exactly one resolved {name} package identity, found {matches:?}"
        ));
    }
    Ok(matches[0].1.to_owned())
}

fn approve_local_package(
    package: &Value,
    workspace_members: &HashSet<&str>,
    trusted_root: &Path,
) -> Result<(), String> {
    let id = package_string(package, "id")?;
    if !workspace_members.contains(id) {
        return Err(format!(
            "unreviewed local dependency: {id} is not a workspace member"
        ));
    }

    let manifest_path = Path::new(package_string(package, "manifest_path")?);
    let canonical_manifest = fs::canonicalize(manifest_path).map_err(|error| {
        format!(
            "canonicalize local dependency manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    if !canonical_manifest.starts_with(trusted_root)
        || canonical_manifest
            .file_name()
            .and_then(|name| name.to_str())
            != Some("Cargo.toml")
        || !canonical_manifest.is_file()
    {
        return Err(format!(
            "unreviewed local dependency manifest for {id}: {} resolves to {} outside the trusted workspace",
            manifest_path.display(),
            canonical_manifest.display()
        ));
    }
    Ok(())
}

pub fn ensure_target(target: &str) -> Result<(), String> {
    if TARGETS.contains(&target) {
        Ok(())
    } else {
        Err(format!("unsupported release target {target:?}"))
    }
}

pub fn metadata(target: Option<&str>) -> Result<Value, String> {
    let root = util::repo_root();
    let mut args = vec![
        "metadata".to_owned(),
        "--locked".to_owned(),
        "--all-features".to_owned(),
        "--format-version".to_owned(),
        "1".to_owned(),
    ];
    if let Some(target) = target {
        args.push("--filter-platform".to_owned());
        args.push(target.to_owned());
    }
    let output = util::command_output("cargo", args, &root)?;
    serde_json::from_str(&output).map_err(|error| format!("parse cargo metadata: {error}"))
}

fn packages(data: &Value) -> Result<&Vec<Value>, String> {
    data.get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata omitted packages".to_owned())
}

fn package_string<'a>(package: &'a Value, key: &str) -> Result<&'a str, String> {
    package
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("cargo package omitted {key}: {package}"))
}

pub fn license_choice(package: &Value) -> Result<&'static str, String> {
    let name = package_string(package, "name")?;
    let version = package_string(package, "version")?;
    let expression = package
        .get("license")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("unreviewed license for {name} {version}: null"))?;

    if name == "backtrace-ext" && version == "0.2.1" {
        return Ok("Apache-2.0");
    }

    let alternatives = expression.split(" OR ").collect::<BTreeSet<_>>();
    if alternatives == BTreeSet::from(["Apache-2.0", "MIT", "Zlib"])
        || alternatives == BTreeSet::from(["Apache-2.0", "BSD-2-Clause", "MIT"])
    {
        return Ok("MIT");
    }

    match expression {
        "MIT OR Apache-2.0"
        | "Apache-2.0 OR MIT"
        | "MIT/Apache-2.0"
        | "MIT"
        | "Unlicense OR MIT"
        | "MIT OR Zlib OR Apache-2.0"
        | "0BSD OR MIT OR Apache-2.0"
        | "MIT OR Apache-2.0 OR LGPL-2.1-or-later"
        | "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT" => Ok("MIT"),
        "Apache-2.0" => Ok("Apache-2.0"),
        "Unicode-3.0" => Ok("Unicode-3.0"),
        "Zlib" => Ok("Zlib"),
        "ISC" => Ok("ISC"),
        "Apache-2.0 WITH LLVM-exception" => Ok("Apache-2.0 WITH LLVM-exception"),
        "(MIT OR Apache-2.0) AND Unicode-3.0" => Ok("MIT AND Unicode-3.0"),
        other => Err(format!(
            "unreviewed license for {name} {version}: {other:?}"
        )),
    }
}

fn closure_packages(data: &Value, root_name: &str) -> Result<Vec<Value>, String> {
    let package_list = packages(data)?;
    let packages_by_id: HashMap<String, Value> = package_list
        .iter()
        .map(|package| Ok((package_string(package, "id")?.to_owned(), package.clone())))
        .collect::<Result<_, String>>()?;

    let nodes = data
        .get("resolve")
        .and_then(|resolve| resolve.get("nodes"))
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata omitted resolve.nodes".to_owned())?;
    let nodes_by_id: HashMap<String, Value> = nodes
        .iter()
        .map(|node| {
            let id = node
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "cargo resolve node omitted id".to_owned())?;
            Ok((id.to_owned(), node.clone()))
        })
        .collect::<Result<_, String>>()?;

    let workspace_members: HashSet<&str> = data
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata omitted workspace_members".to_owned())?
        .iter()
        .filter_map(Value::as_str)
        .collect();

    let roots: Vec<String> = package_list
        .iter()
        .filter_map(|package| {
            let id = package.get("id")?.as_str()?;
            (package.get("name")?.as_str()? == root_name && workspace_members.contains(id))
                .then(|| id.to_owned())
        })
        .collect();
    if roots.len() != 1 {
        return Err(format!(
            "expected one workspace {root_name} package, found {}",
            roots.len()
        ));
    }

    let mut pending = roots;
    let mut seen = HashSet::new();
    while let Some(package_id) = pending.pop() {
        if !seen.insert(package_id.clone()) {
            continue;
        }
        let node = nodes_by_id
            .get(&package_id)
            .ok_or_else(|| format!("missing resolve node for {package_id}"))?;
        let deps = node
            .get("deps")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("resolve node omitted deps for {package_id}"))?;
        for dependency in deps {
            let include = dependency
                .get("dep_kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| {
                    kinds.iter().any(|kind| {
                        kind.get("kind").is_some_and(Value::is_null)
                            || kind.get("kind").and_then(Value::as_str) == Some("build")
                    })
                });
            if include {
                let id = dependency
                    .get("pkg")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("dependency omitted pkg for {package_id}"))?;
                pending.push(id.to_owned());
            }
        }
    }

    let mut native = seen
        .into_iter()
        .map(|id| {
            packages_by_id
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("missing package metadata for {id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    native.sort_by_key(|package| {
        (
            package
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            package
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )
    });
    Ok(native)
}

pub fn native_packages(data: &Value) -> Result<Vec<Value>, String> {
    closure_packages(data, "wit-language-server")
}

pub fn check(data: &Value) -> Result<Vec<Value>, String> {
    let package_list = packages(data)?;
    let grammar_revision = pinned_grammar_revision()?;
    let expected_grammar_source =
        format!("git+{GRAMMAR_REPOSITORY}?rev={grammar_revision}#{grammar_revision}");
    let grammar_count = package_list
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some("tree-sitter-wit"))
        .count();
    if grammar_count != 1 {
        return Err(format!(
            "expected one tree-sitter-wit package in Cargo metadata, found {grammar_count}"
        ));
    }

    let trusted_root = fs::canonicalize(util::repo_root())
        .map_err(|error| format!("canonicalize repository root: {error}"))?;
    let workspace_members = data
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata omitted workspace_members".to_owned())?
        .iter()
        .map(|member| {
            member
                .as_str()
                .ok_or_else(|| "invalid workspace member ID".to_owned())
        })
        .collect::<Result<HashSet<_>, _>>()?;

    for package in package_list {
        license_choice(package)?;
        let name = package_string(package, "name")?;
        match package.get("source") {
            Some(Value::String(source)) if name == "tree-sitter-wit" => {
                if source != &expected_grammar_source {
                    return Err(format!(
                        "unreviewed tree-sitter-wit source: {source:?}; expected {expected_grammar_source:?}"
                    ));
                }
            }
            Some(Value::String(source)) if source == CRATES_IO_SOURCE => {}
            Some(Value::Null) if name != "tree-sitter-wit" => {
                approve_local_package(package, &workspace_members, &trusted_root)?;
            }
            Some(Value::String(source)) if name == "tree-sitter-wit" => {
                return Err(format!(
                    "unreviewed tree-sitter-wit source: {source:?}; expected {expected_grammar_source:?}"
                ));
            }
            Some(Value::String(source)) => {
                return Err(format!("unreviewed dependency source: {source}"));
            }
            _ => {
                return Err(format!("missing or malformed dependency source for {name}"));
            }
        }
    }

    let native = native_packages(data)?;
    single_version(&native, "wit-parser", "native language server")?;
    let adapter = closure_packages(data, "zed-wit")?;
    single_version(&adapter, "wit-parser", "Zed extension adapter")?;
    single_version(package_list, "tree-sitter", "workspace")?;
    Ok(native)
}

pub fn run(target: Option<&str>) -> Result<(), String> {
    let data = metadata(target)?;
    let native = check(&data)?;
    let package_list = packages(&data)?;
    let adapter = closure_packages(&data, "zed-wit")?;
    let licenses: BTreeSet<&str> = package_list
        .iter()
        .map(license_choice)
        .collect::<Result<_, _>>()?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "target": target.unwrap_or("all-platforms"),
            "workspace_packages": package_list.len(),
            "native_and_build_packages": native.len(),
            "native_wit_parser": single_version(&native, "wit-parser", "native language server")?,
            "adapter_binding_wit_parser": single_version(&adapter, "wit-parser", "Zed extension adapter")?,
            "tree_sitter": single_version(package_list, "tree-sitter", "workspace")?,
            "licenses": licenses,
            "result": "passed",
        }))
        .map_err(|error| format!("serialize dependency report: {error}"))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_mit_apache_zlib_alternatives_are_order_independent() {
        for expression in [
            "MIT OR Zlib OR Apache-2.0",
            "Zlib OR Apache-2.0 OR MIT",
            "Apache-2.0 OR MIT OR Zlib",
        ] {
            let package = json!({
                "name": "license-order-fixture",
                "version": "1.0.0",
                "license": expression,
            });
            assert_eq!(license_choice(&package).unwrap(), "MIT");
        }
    }

    #[test]
    fn approved_mit_apache_bsd2_alternatives_are_order_independent() {
        for expression in [
            "BSD-2-Clause OR Apache-2.0 OR MIT",
            "MIT OR BSD-2-Clause OR Apache-2.0",
            "Apache-2.0 OR MIT OR BSD-2-Clause",
        ] {
            let package = json!({
                "name": "license-order-fixture",
                "version": "1.0.0",
                "license": expression,
            });
            assert_eq!(license_choice(&package).unwrap(), "MIT");
        }
    }
    fn fake_package(id: &str, name: &str, version: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "version": version,
            "license": "MIT",
            "source": CRATES_IO_SOURCE,
        })
    }

    fn graph_fixture(native_parser: &str, runtime: &str) -> Value {
        let revision = pinned_grammar_revision().unwrap();
        let mut grammar = fake_package("grammar", "tree-sitter-wit", "1.4.0");
        grammar["source"] = json!(format!(
            "git+{GRAMMAR_REPOSITORY}?rev={revision}#{revision}"
        ));
        let root = util::repo_root();
        let mut native = fake_package("native", "wit-language-server", "0.1.2");
        native["source"] = Value::Null;
        native["manifest_path"] = json!(root.join("crates/wit-language-server/Cargo.toml"));
        let mut adapter = fake_package("adapter", "zed-wit", "0.5.4");
        adapter["source"] = Value::Null;
        adapter["manifest_path"] = json!(root.join("Cargo.toml"));
        let mut syntax = fake_package("syntax", "wit-syntax", "0.1.0");
        syntax["source"] = Value::Null;
        syntax["manifest_path"] = json!(root.join("crates/wit-syntax/Cargo.toml"));
        json!({
            "packages": [
                native,
                adapter,
                syntax,
                fake_package("native-parser", "wit-parser", native_parser),
                fake_package("adapter-parser", "wit-parser", "0.227.1"),
                fake_package("runtime", "tree-sitter", runtime),
                grammar,
                fake_package("dev", "unused-dev-dependency", "1.0.0"),
            ],
            "workspace_members": ["native", "adapter", "syntax"],
            "resolve": {
                "nodes": [
                    {"id": "native", "deps": [
                        {"pkg": "native-parser", "dep_kinds": [{"kind": null}]},
                        {"pkg": "runtime", "dep_kinds": [{"kind": "build"}]},
                        {"pkg": "syntax", "dep_kinds": [{"kind": null}]},
                        {"pkg": "dev", "dep_kinds": [{"kind": "dev"}]},
                    ]},
                    {"id": "adapter", "deps": [
                        {"pkg": "adapter-parser", "dep_kinds": [{"kind": null}]},
                    ]},
                    {"id": "syntax", "deps": [
                        {"pkg": "grammar", "dep_kinds": [{"kind": null}]},
                    ]},
                    {"id": "native-parser", "deps": []},
                    {"id": "adapter-parser", "deps": []},
                    {"id": "runtime", "deps": []},
                    {"id": "grammar", "deps": []},
                    {"id": "dev", "deps": []},
                ],
            },
        })
    }

    #[test]
    fn normal_parser_and_runtime_upgrades_do_not_require_xtask_changes() {
        let data = graph_fixture("0.261.0", "0.26.13");
        let native = check(&data).unwrap();
        assert_eq!(
            single_version(&native, "wit-parser", "native").unwrap(),
            "0.261.0"
        );
        let adapter = closure_packages(&data, "zed-wit").unwrap();
        assert_eq!(
            single_version(&adapter, "wit-parser", "adapter").unwrap(),
            "0.227.1"
        );
    }

    #[test]
    fn native_graph_excludes_dev_only_edges_but_includes_build_dependencies() {
        let data = graph_fixture("0.261.0", "0.26.13");
        let native = native_packages(&data).unwrap();
        assert!(native.iter().any(|p| p["name"] == "tree-sitter"));
        assert!(!native.iter().any(|p| p["name"] == "unused-dev-dependency"));
    }

    #[test]
    fn duplicate_native_semantic_parsers_are_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"].as_array_mut().unwrap().push(fake_package(
            "other-parser",
            "wit-parser",
            "0.262.0",
        ));
        data["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "other-parser", "deps": []}));
        data["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"pkg": "other-parser", "dep_kinds": [{"kind": null}]}));
        assert!(check(&data).unwrap_err().contains("native language server"));
    }

    #[test]
    fn duplicate_tree_sitter_runtime_versions_are_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"].as_array_mut().unwrap().push(fake_package(
            "other-runtime",
            "tree-sitter",
            "0.27.0",
        ));
        assert!(check(&data).unwrap_err().contains("workspace"));
    }

    #[test]
    fn unreviewed_registry_or_git_sources_are_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][3]["source"] = json!("git+https://example.com/unreviewed#123");
        assert!(
            check(&data)
                .unwrap_err()
                .contains("unreviewed dependency source")
        );
    }

    #[test]
    fn approved_local_workspace_members_pass() {
        check(&graph_fixture("0.261.0", "0.26.13")).unwrap();
    }

    #[test]
    fn external_path_dependency_is_rejected_even_if_declared_a_workspace_member() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][0]["manifest_path"] = json!(std::env::temp_dir());
        assert!(
            check(&data)
                .unwrap_err()
                .contains("outside the trusted workspace")
        );
    }

    #[test]
    fn in_repo_path_package_not_in_workspace_is_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][7]["source"] = Value::Null;
        data["packages"][7]["manifest_path"] = json!(util::repo_root().join("Cargo.toml"));
        assert!(check(&data).unwrap_err().contains("not a workspace member"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_manifest_outside_repo_is_rejected() {
        use std::os::unix::fs::symlink;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "dependency-policy-{}-{nonce}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let outside = std::env::temp_dir().join(format!("{name}-outside"));
        let inside = util::repo_root()
            .join("target")
            .join(format!("{name}-link"));
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(inside.parent().unwrap()).unwrap();
        fs::write(
            outside.join("Cargo.toml"),
            "[package]\nname = \"external\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        symlink(&outside, &inside).unwrap();
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][0]["manifest_path"] = json!(inside.join("Cargo.toml"));
        let result = check(&data);
        fs::remove_file(&inside).unwrap();
        fs::remove_dir_all(&outside).unwrap();
        assert!(
            result
                .unwrap_err()
                .contains("outside the trusted workspace")
        );
    }

    #[test]
    fn identical_versions_with_distinct_native_parser_package_ids_are_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"].as_array_mut().unwrap().push(fake_package(
            "other-parser",
            "wit-parser",
            "0.261.0",
        ));
        data["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "other-parser", "deps": []}));
        data["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"pkg": "other-parser", "dep_kinds": [{"kind": null}]}));
        assert!(check(&data).unwrap_err().contains("package identity"));
    }

    #[test]
    fn identical_versions_with_distinct_tree_sitter_package_ids_are_rejected() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"].as_array_mut().unwrap().push(fake_package(
            "other-runtime",
            "tree-sitter",
            "0.26.13",
        ));
        assert!(check(&data).unwrap_err().contains("package identity"));
    }

    #[test]
    fn exact_registry_source_passes_and_prefix_lookalikes_fail() {
        check(&graph_fixture("0.261.0", "0.26.13")).unwrap();
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][3]["source"] = json!(format!("{CRATES_IO_SOURCE}-evil"));
        assert!(
            check(&data)
                .unwrap_err()
                .contains("unreviewed dependency source")
        );
        data["packages"][3]["source"] = json!("registry+https://example.org/other-index");
        assert!(
            check(&data)
                .unwrap_err()
                .contains("unreviewed dependency source")
        );
    }

    #[test]
    fn resolved_grammar_git_commit_must_match_manifest_pin() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][6]["source"] = json!(
            "git+https://github.com/bytecodealliance/tree-sitter-wit?rev=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(
            check(&data)
                .unwrap_err()
                .contains("unreviewed tree-sitter-wit source")
        );
    }

    #[test]
    fn mismatched_or_invalid_grammar_manifest_pins_are_rejected() {
        let root = util::repo_root();
        let syntax = util::read_nonempty(&root.join("crates/wit-syntax/Cargo.toml")).unwrap();
        let extension = util::read_nonempty(&root.join("extension.toml")).unwrap();
        let revision = grammar_revision(&syntax, &extension).unwrap();
        assert!(
            grammar_revision(
                &syntax,
                &extension.replace(&revision, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            )
            .unwrap_err()
            .contains("grammar pin mismatch")
        );
        assert!(
            grammar_revision(
                &syntax.replace(&revision, "not-a-git-revision"),
                &extension.replace(&revision, "not-a-git-revision")
            )
            .unwrap_err()
            .contains("invalid tree-sitter-wit commit revision")
        );
    }

    #[test]
    fn unreviewed_licenses_are_rejected_even_for_valid_dependency_versions() {
        let mut data = graph_fixture("0.261.0", "0.26.13");
        data["packages"][3]["license"] = json!("GPL-3.0-only");
        assert!(check(&data).unwrap_err().contains("unreviewed license"));
    }

    #[test]
    fn release_target_allowlist_is_preserved() {
        for target in TARGETS {
            assert!(ensure_target(target).is_ok());
        }
        assert!(ensure_target("aarch64-pc-windows-msvc").is_err());
    }
}
