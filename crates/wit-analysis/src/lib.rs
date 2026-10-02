//! WIT package analysis and concrete-syntax formatting, independent of LSP.
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};
use wit_parser::{Resolve, SourceMap, UnresolvedPackageGroup};

/// Open buffers take precedence over files on disk.
pub type Overlays = BTreeMap<PathBuf, String>;

/// A parser or resolver error with an exact UTF-8 byte range.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub message: String,
}

/// IO and formatting failures remain explicit instead of appearing valid.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("analysis failed: {0}")]
    Analysis(String),
    #[error("formatting failed: {0}")]
    Format(String),
}

fn files(directory: &Path, overlays: &Overlays) -> Result<BTreeSet<PathBuf>, Error> {
    let mut paths = BTreeSet::new();
    match std::fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    path: directory.into(),
                    source,
                })?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "wit") && path.is_file() {
                    paths.insert(path);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                path: directory.into(),
                source,
            });
        }
    }
    paths.extend(
        overlays
            .keys()
            .filter(|p| p.parent() == Some(directory) && p.extension().is_some_and(|e| e == "wit"))
            .cloned(),
    );
    Ok(paths)
}

fn parse(
    paths: BTreeSet<PathBuf>,
    overlays: &Overlays,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<UnresolvedPackageGroup>, Error> {
    if paths.is_empty() {
        return Ok(None);
    }
    let fallback = paths.first().cloned();
    let mut map = SourceMap::new();
    for path in paths {
        let source = match overlays.get(&path) {
            Some(source) => source.clone(),
            None => std::fs::read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?,
        };
        map.push(&path, source);
    }
    match map.parse() {
        Ok(group) => Ok(Some(group)),
        Err((map, error)) => {
            if let Some(location) = map.resolve_span(error.kind().span()) {
                diagnostics.push(Diagnostic {
                    path: location.path.into(),
                    range: location.range,
                    message: error.to_string(),
                });
            } else if let Some(path) = fallback {
                diagnostics.push(Diagnostic {
                    path,
                    range: 0..0,
                    message: error.to_string(),
                });
            }
            Ok(None)
        }
    }
}

/// Analyze sibling files and files/package directories immediately below `deps/`.
/// No workspace traversal or disk mutation occurs.
///
/// # Errors
/// Returns an IO error when a discovered source cannot be read.
pub fn analyze(directory: &Path, overlays: &Overlays) -> Result<Vec<Diagnostic>, Error> {
    let mut diagnostics = Vec::new();
    let main_paths = files(directory, overlays)?;
    let fallback = main_paths.first().cloned();
    let main = parse(main_paths, overlays, &mut diagnostics)?;
    let deps_dir = directory.join("deps");
    let mut dep_paths: BTreeSet<PathBuf> = files(&deps_dir, overlays)?;
    match std::fs::read_dir(&deps_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    path: deps_dir.clone(),
                    source,
                })?;
                if entry
                    .file_type()
                    .map_err(|source| Error::Io {
                        path: entry.path(),
                        source,
                    })?
                    .is_dir()
                {
                    dep_paths.insert(entry.path());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                path: deps_dir,
                source,
            });
        }
    }
    for path in overlays.keys() {
        if let Some(parent) = path.parent()
            && parent.parent() == Some(deps_dir.as_path())
        {
            dep_paths.insert(parent.into());
        }
    }
    let mut dependencies = Vec::new();
    for path in dep_paths {
        let paths = if path.extension().is_some_and(|e| e == "wit") && !path.is_dir() {
            BTreeSet::from([path])
        } else {
            files(&path, overlays)?
        };
        if let Some(group) = parse(paths, overlays, &mut diagnostics)? {
            dependencies.push(group);
        }
    }
    if diagnostics.is_empty()
        && let Some(main) = main
    {
        let mut resolve = Resolve::default();
        if let Err(error) = resolve.push_groups(main, dependencies) {
            if let Some(location) = resolve.source_map.resolve_span(error.kind().span()) {
                diagnostics.push(Diagnostic {
                    path: location.path.into(),
                    range: location.range,
                    message: error.to_string(),
                });
            } else {
                diagnostics.push(Diagnostic {
                    path: fallback
                        .clone()
                        .ok_or_else(|| Error::Analysis(error.to_string()))?,
                    range: 0..0,
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(diagnostics)
}

/// Format with upstream Topiary WIT queries, rejecting syntax errors and checking idempotence.
///
/// # Errors
/// Returns a formatting error for unsupported syntax or non-idempotent output.
pub fn format(source: &str) -> Result<String, Error> {
    format_with_indent(source, "    ")
}

/// Format with the requested spaces or tab indentation.
///
/// # Errors
/// Returns an error for invalid indentation, unsupported syntax or non-idempotent output.
pub fn format_with_indent(source: &str, indent: &str) -> Result<String, Error> {
    if indent.is_empty() || indent.len() > 16 || !indent.bytes().all(|b| b == b' ' || b == b'\t') {
        return Err(Error::Format(
            "indent must contain 1 to 16 spaces or tabs".into(),
        ));
    }
    let grammar = wit_syntax::language().into();
    let query = topiary_core::TopiaryQuery::new(&grammar, include_str!("../queries/wit.scm"))
        .map_err(|e| Error::Format(format!("{e:?}")))?;
    let language = topiary_core::Language {
        name: "wit".into(),
        query,
        grammar,
        indent: Some(indent.into()),
    };
    let mut output = Vec::new();
    topiary_core::formatter_str(
        source,
        &mut output,
        &language,
        topiary_core::Operation::Format {
            skip_idempotence: false,
            tolerate_parsing_errors: false,
        },
    )
    .map_err(|e| Error::Format(format!("{e:?}")))?;
    String::from_utf8(output).map_err(|e| Error::Format(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn siblings_overlays_and_structured_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wit");
        let b = dir.path().join("b.wit");
        std::fs::write(&a, "package test:app; interface a { use b.{thing}; }").unwrap();
        std::fs::write(&b, "interface b { type thing = u32; }").unwrap();
        assert!(analyze(dir.path(), &Overlays::new()).unwrap().is_empty());
        let bad = "// 🦀\ninterface b { type thing = missing; }";
        let overlays = Overlays::from([(b.clone(), bad.into())]);
        let errors = analyze(dir.path(), &overlays).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, b);
        assert_eq!(&bad[errors[0].range.clone()], "missing");
    }
    #[test]
    fn dependencies_resolve_topologically() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("deps/a")).unwrap();
        std::fs::write(
            dir.path().join("main.wit"),
            "package test:app; world app { import test:a/api; }",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("deps/a/a.wit"),
            "package test:a; interface api { use test:z/api.{thing}; }",
        )
        .unwrap();
        let dep = dir.path().join("deps/z.wit");
        std::fs::write(&dep, "package test:z; interface api { type thing = u32; }").unwrap();
        assert!(analyze(dir.path(), &Overlays::new()).unwrap().is_empty());
        let errors = analyze(
            dir.path(),
            &Overlays::from([(dep.clone(), "package test:z; interface api {}".into())]),
        )
        .unwrap();
        assert!(!errors.is_empty());
    }
    #[test]
    fn invalid_package_declarations_and_duplicate_dependencies_report_sources() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.wit");
        for source in [
            "package test:app; package test:other; interface api {}",
            "package test:app; interface api {} interface api {}",
        ] {
            let errors =
                analyze(dir.path(), &Overlays::from([(main.clone(), source.into())])).unwrap();
            assert!(!errors.is_empty());
            assert_eq!(errors[0].path, main);
        }
        std::fs::create_dir(dir.path().join("deps")).unwrap();
        std::fs::write(
            &main,
            "package test:app; world app { import test:dep/api; }",
        )
        .unwrap();
        for name in ["a.wit", "b.wit"] {
            std::fs::write(
                dir.path().join("deps").join(name),
                "package test:dep; interface api {}",
            )
            .unwrap();
        }
        let errors = analyze(dir.path(), &Overlays::new()).unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("defined"));
        assert!(errors[0].path.starts_with(dir.path().join("deps")));
    }
    #[test]
    fn exact_version_dependencies_and_nested_packages_use_parser_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("deps")).unwrap();
        let main = dir.path().join("main.wit");
        std::fs::write(
            &main,
            "package test:app; world app { import test:dep/api@1.2.3; }",
        )
        .unwrap();
        let dep = dir.path().join("deps/dep.wit");
        std::fs::write(&dep, "package test:dep@1.2.3; interface api {}").unwrap();
        assert!(analyze(dir.path(), &Overlays::new()).unwrap().is_empty());
        std::fs::write(&dep, "package test:dep@2.0.0; interface api {}").unwrap();
        let errors = analyze(dir.path(), &Overlays::new()).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, main);
        assert!(errors[0].message.contains("1.2.3"));
        std::fs::remove_file(dep).unwrap();
        std::fs::write(
            &main,
            include_str!("../../../tests/fixtures/gated/nested-packages/nested-packages.wit"),
        )
        .unwrap();
        assert!(analyze(dir.path(), &Overlays::new()).unwrap().is_empty());
    }
    #[test]
    fn non_ascii_identifier_is_rejected_at_upstream_identifier_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.wit");
        let source = "package test:app; interface api { type café = u32; }";
        let errors = analyze(dir.path(), &Overlays::from([(main.clone(), source.into())])).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, main);
        let start = source.find("café").unwrap();
        assert_eq!(errors[0].range, start..start + 1);
        assert!(
            errors[0]
                .message
                .contains("invalid character in identifier 'é'")
        );
    }
    #[test]
    fn formatter_preserves_doc_and_nested_block_comments() {
        let source = "package test:app;\n/// café 🦀 docs\ninterface api {/* outer /* nested */ comment */call:func();}";
        let formatted = format(source).unwrap();
        assert!(formatted.contains("/// café 🦀 docs"));
        assert!(formatted.contains("/* outer /* nested */ comment */"));
        assert_eq!(format(&formatted).unwrap(), formatted);
    }
    #[test]
    fn fixture_packages_resolve_with_upstream_parser() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for package in [
            "current/annotations",
            "current/async",
            "current/core",
            "gated/features",
            "gated/nested-packages",
        ] {
            let directory = root.join("tests/fixtures").join(package);
            let diagnostics = analyze(&directory, &Overlays::new()).unwrap();
            assert!(
                diagnostics.is_empty(),
                "{package}: {}",
                diagnostics
                    .iter()
                    .map(|diagnostic| format!(
                        "{}: {}",
                        diagnostic.path.display(),
                        diagnostic.message
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }

    #[test]
    fn grammar_gap_fixtures_match_upstream_parser_behavior() {
        for (name, source, expected_clean) in [
            (
                "getters-setters.wit",
                include_str!(
                    "../../../tests/fixtures/grammar-gaps/getters-setters/getters-setters.wit"
                ),
                true,
            ),
            (
                "legacy-named-results.wit",
                include_str!(
                    "../../../tests/fixtures/grammar-gaps/legacy-named-results/legacy-named-results.wit"
                ),
                false,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(name), source).unwrap();
            let diagnostics = analyze(dir.path(), &Overlays::new()).unwrap();
            assert_eq!(
                diagnostics.is_empty(),
                expected_clean,
                "{name}: {}",
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
    }

    #[test]
    fn modern_fixture_formatting_is_idempotent() {
        for (name, source) in [
            (
                "core",
                include_str!("../../../tests/fixtures/current/core/core.wit"),
            ),
            (
                "async",
                include_str!("../../../tests/fixtures/current/async/async.wit"),
            ),
            (
                "annotations",
                include_str!("../../../tests/fixtures/current/annotations/annotations.wit"),
            ),
            (
                "features",
                include_str!("../../../tests/fixtures/gated/features/features.wit"),
            ),
            (
                "nested",
                include_str!("../../../tests/fixtures/gated/nested-packages/nested-packages.wit"),
            ),
        ] {
            let formatted = format(source).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(format(&formatted).unwrap(), formatted, "{name}");
        }
    }
    #[test]
    fn formatter_preserves_comments_and_is_idempotent() {
        let source = "package test:app;\n// 🦀 comment\ninterface api{record thing{x:u32,y:string} call:func(x:thing)->string;}";
        let formatted = format(source).unwrap();
        assert!(formatted.contains("// 🦀 comment"));
        assert_eq!(format(&formatted).unwrap(), formatted);
        assert!(format("interface {").is_err());
    }
}
