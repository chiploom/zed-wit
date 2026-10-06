use crate::util;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};

pub const TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

const GRAMMAR_REV: &str = "cdf07263b136054b413cab449ac7a1d059c27542";

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
    if alternatives == BTreeSet::from(["Apache-2.0", "MIT", "Zlib"]) {
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

pub fn native_packages(data: &Value) -> Result<Vec<Value>, String> {
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
            (package.get("name")?.as_str()? == "wit-language-server"
                && workspace_members.contains(id))
            .then(|| id.to_owned())
        })
        .collect();
    if roots.len() != 1 {
        return Err(format!(
            "expected one workspace wit-language-server package, found {}",
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

pub fn check(data: &Value) -> Result<Vec<Value>, String> {
    let package_list = packages(data)?;

    for package in package_list {
        license_choice(package)?;
        if let Some(source) = package.get("source").and_then(Value::as_str)
            && !source.starts_with("registry+https://github.com/rust-lang/crates.io-index")
        {
            let name = package_string(package, "name")?;
            let expected_git = "git+https://github.com/bytecodealliance/tree-sitter-wit?";
            let expected_revision = format!("#{GRAMMAR_REV}");
            if name != "tree-sitter-wit"
                || !source.starts_with(expected_git)
                || !source.ends_with(&expected_revision)
            {
                return Err(format!("unreviewed dependency source: {source}"));
            }
        }
    }

    let native = native_packages(data)?;
    let native_parsers: BTreeSet<&str> = native
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some("wit-parser"))
        .filter_map(|package| package.get("version").and_then(Value::as_str))
        .collect();
    if native_parsers != BTreeSet::from(["0.260.0"]) {
        return Err(format!("native semantic parser drift: {native_parsers:?}"));
    }

    let tree_sitters: BTreeSet<&str> = package_list
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some("tree-sitter"))
        .filter_map(|package| package.get("version").and_then(Value::as_str))
        .collect();
    if tree_sitters != BTreeSet::from(["0.26.11"]) {
        return Err(format!("Tree-sitter runtime drift: {tree_sitters:?}"));
    }

    let parser_versions: BTreeSet<&str> = package_list
        .iter()
        .filter(|package| package.get("name").and_then(Value::as_str) == Some("wit-parser"))
        .filter_map(|package| package.get("version").and_then(Value::as_str))
        .collect();
    if parser_versions != BTreeSet::from(["0.227.1", "0.260.0"]) {
        return Err(format!(
            "workspace parser versions changed; review adapter bindings: {parser_versions:?}"
        ));
    }

    Ok(native)
}

pub fn run(target: Option<&str>) -> Result<(), String> {
    let data = metadata(target)?;
    let native = check(&data)?;
    let package_list = packages(&data)?;
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
            "native_wit_parser": "0.260.0",
            "adapter_binding_wit_parser": "0.227.1",
            "tree_sitter": "0.26.11",
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
}
