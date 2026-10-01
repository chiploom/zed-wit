use std::{collections::BTreeSet, fs, path::PathBuf};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser.set_language(&wit_syntax::language()).unwrap();
    parser.parse(source, None).unwrap()
}

fn query(name: &str) -> Query {
    let source = fs::read_to_string(root().join(format!("languages/wit/{name}.scm"))).unwrap();
    Query::new(&wit_syntax::language(), &source)
        .unwrap_or_else(|error| panic!("{name}.scm: {error}"))
}

fn captures(name: &str, source: &str) -> Vec<(String, String, usize)> {
    let tree = parse(source);
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let query = query(name);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut found = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures {
            found.push((
                query.capture_names()[capture.index as usize].to_string(),
                source[capture.node.byte_range()].to_string(),
                capture.node.start_byte(),
            ));
        }
    }
    found
}

fn texts(name: &str, source: &str, capture_name: &str) -> Vec<String> {
    captures(name, source)
        .into_iter()
        .filter(|(capture, _, _)| capture == capture_name)
        .map(|(_, text, _)| text)
        .collect()
}

#[test]
fn all_queries_compile_with_only_zed_supported_captures() {
    let highlights = [
        "attribute",
        "comment",
        "comment.doc",
        "constant",
        "constant.builtin",
        "constructor",
        "enum",
        "function",
        "keyword",
        "number",
        "operator",
        "property",
        "punctuation.bracket",
        "punctuation.delimiter",
        "punctuation.special",
        "string",
        "string.special",
        "type",
        "type.builtin",
        "variable",
        "variable.parameter",
        "variant",
    ];
    for (name, allowed) in [
        ("highlights", highlights.as_slice()),
        ("brackets", &["open", "close"][..]),
        ("indents", &["indent", "start", "end", "outdent"][..]),
        (
            "outline",
            &["item", "name", "context", "context.extra", "annotation"][..],
        ),
        ("overrides", &["comment.inclusive", "string"][..]),
        (
            "textobjects",
            &[
                "class.around",
                "class.inside",
                "function.around",
                "function.inside",
                "comment.around",
                "comment.inside",
            ][..],
        ),
    ] {
        let query = query(name);
        assert!(query.pattern_count() > 0, "empty {name} query");
        for capture in query.capture_names() {
            assert!(
                allowed.contains(capture),
                "unsupported {name} capture: {capture}"
            );
        }
        println!(
            "compiled {name}: {} patterns, {} captures",
            query.pattern_count(),
            query.capture_names().len()
        );
    }
    assert_eq!(wit_syntax::language().abi_version(), 15);
}

#[test]
fn current_and_gated_corpus_parse_without_errors() {
    for group in ["current", "gated"] {
        let dir = root().join("tests/fixtures").join(group);
        let mut paths: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        assert!(!paths.is_empty());
        for path in paths {
            let source = fs::read_to_string(&path).unwrap();
            let tree = parse(&source);
            assert!(
                !tree.root_node().has_error(),
                "{}: {}",
                path.display(),
                tree.root_node().to_sexp()
            );
            for name in [
                "highlights",
                "brackets",
                "indents",
                "outline",
                "textobjects",
                "overrides",
            ] {
                let found = captures(name, &source);
                if name != "overrides" {
                    assert!(!found.is_empty(), "{}: no {name} captures", path.display());
                }
            }
            println!(
                "error-free {group}/{} ({} bytes)",
                path.file_name().unwrap().to_string_lossy(),
                source.len()
            );
        }
    }
}

#[test]
fn grammar_limitations_are_explicit() {
    let gaps = root().join("tests/fixtures/grammar-gaps");
    let sugar = fs::read_to_string(gaps.join("getters-setters.wit")).unwrap();
    assert!(
        parse(&sugar).root_node().has_error(),
        "getter/setter support changed: update qualification"
    );
    let legacy = fs::read_to_string(gaps.join("legacy-named-results.wit")).unwrap();
    assert!(
        !parse(&legacy).root_node().has_error(),
        "legacy result behavior changed: update qualification"
    );
    println!(
        "confirmed grammar gaps: getter/setter sugar rejected; obsolete named results accepted"
    );
}

#[test]
fn highlights_distinguish_parameters_fields_methods_annotations_and_strings() {
    let source = r#"package demo:editing@1.0.0;
/// Unicode 🧩 docs
interface api {
    record entry { value: string }
    resource file {
        constructor(path: string);
        open: static func(path: string) -> file;
        read: func(count: u32) -> list<u8, 4>;
    }
    @since(version = 1.0.0)
    @unstable(feature = experimental)
    @deprecated(version = 2.0.0)
    @external-id("café/\u{1f9e9}")
    %type: async func(value: entry) -> future<stream<u8>>;
}"#;
    assert_eq!(
        texts("highlights", source, "function"),
        ["open", "read", "%type"]
    );
    assert_eq!(texts("highlights", source, "constructor"), ["constructor"]);
    assert_eq!(
        texts("highlights", source, "variable.parameter"),
        ["path", "path", "count", "value"]
    );
    assert_eq!(
        texts("highlights", source, "property"),
        ["value", "version", "feature", "version"]
    );
    assert_eq!(
        texts("highlights", source, "attribute"),
        ["since", "unstable", "deprecated", "external-id"]
    );
    assert_eq!(
        texts("highlights", source, "string"),
        [r#""café/\u{1f9e9}""#]
    );
    assert_eq!(texts("highlights", source, "number"), ["4"]);
    assert_eq!(
        texts("highlights", source, "comment.doc"),
        ["/// Unicode 🧩 docs\n"]
    );
    let types = texts("highlights", source, "type");
    assert!(types.contains(&"entry".into()) && types.contains(&"file".into()));
}

#[test]
fn outline_and_function_objects_do_not_duplicate_resource_methods() {
    let source = "package demo:editing; interface api { record entry { field: u32 } resource file { constructor(); open: static func() -> file; read: func(size: u32); } call: func(value: entry); } world app { import api; export run: func(); }";
    assert_eq!(
        texts("outline", source, "name"),
        [
            "package demo:editing",
            "api",
            "entry",
            "field",
            "file",
            "constructor",
            "open",
            "read",
            "call",
            "app",
            "api",
            "run"
        ]
    );
    let items = texts("outline", source, "item");
    assert_eq!(items.len(), 12);
    let functions = texts("textobjects", source, "function.around");
    assert_eq!(
        functions,
        [
            "constructor();",
            "open: static func() -> file;",
            "read: func(size: u32);",
            "call: func(value: entry);",
            "export run: func();"
        ]
    );
    assert!(!texts("textobjects", source, "class.inside").is_empty());
    println!("outline: 12 unique items; function navigation: 5 unique declarations");
}

#[test]
fn bracket_pairs_and_indent_ranges_cover_nested_multiline_constructs() {
    let source = "package demo:nested {\ninterface api {\nuse other.{\nvalue,\n};\nresource file {\nread: func(\ninput: borrow<file>,\n) -> result<\nlist<map<string, tuple<u8, u32>>>,\noption<future<stream<u8>>>\n>;\n}\n}\n}";
    let tree = parse(source);
    assert!(!tree.root_node().has_error());
    let query = query("brackets");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut pairs = 0;
    while let Some(matched) = matches.next() {
        assert_eq!(matched.captures.len(), 2);
        let a = matched.captures[0].node;
        let b = matched.captures[1].node;
        assert!(a.end_byte() <= b.start_byte());
        assert!(matches!(
            (&source[a.byte_range()], &source[b.byte_range()]),
            ("{", "}") | ("(", ")") | ("<", ">")
        ));
        pairs += 1;
    }
    assert_eq!(pairs, 13);
    let ranges = captures("indents", source);
    assert_eq!(
        ranges
            .iter()
            .filter(|(name, _, _)| name == "indent")
            .count(),
        pairs
    );
    assert_eq!(
        ranges.iter().filter(|(name, _, _)| name == "start").count(),
        pairs
    );
    assert_eq!(
        ranges.iter().filter(|(name, _, _)| name == "end").count(),
        pairs
    );
    println!("paired {pairs} delimiters with corresponding indentation boundaries");
}

#[test]
fn comments_and_strings_disable_auto_closing_scopes() {
    let source = "// { \" ignored at line end\ninterface api { /* ( < */ @external-id(\"({<🧩>})\") call: func(); }";
    assert_eq!(
        texts("overrides", source, "comment.inclusive"),
        ["// { \" ignored at line end", "/* ( < */"]
    );
    assert_eq!(texts("overrides", source, "string"), ["\"({<🧩>})\""]);
    assert_eq!(texts("brackets", source, "open").len(), 3);
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(root().join("languages/wit/config.toml")).unwrap())
            .unwrap();
    assert_eq!(config["name"].as_str(), Some("WIT"));
    assert_eq!(config["grammar"].as_str(), Some("wit"));
    assert_eq!(config["tab_size"].as_integer(), Some(4));
    assert_eq!(config["hard_tabs"].as_bool(), Some(false));
    assert_eq!(
        config["path_suffixes"].as_array().unwrap(),
        &[toml::Value::String("wit".into())]
    );
    assert_eq!(config["line_comments"][0].as_str(), Some("// "));
    assert_eq!(config["block_comment"][0].as_str(), Some("/*"));
    assert_eq!(config["block_comment"][1].as_str(), Some("*/"));
    let mut delimiters = BTreeSet::new();
    for pair in config["brackets"].as_array().unwrap() {
        assert_eq!(pair["close"].as_bool(), Some(true));
        let disabled: Vec<_> = pair["not_in"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect();
        assert_eq!(disabled, ["comment", "string"]);
        delimiters.insert((
            pair["start"].as_str().unwrap(),
            pair["end"].as_str().unwrap(),
        ));
    }
    assert_eq!(
        delimiters,
        BTreeSet::from([("{", "}"), ("(", ")"), ("<", ">"), ("\"", "\"")])
    );
}

fn expand_defaults(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            let placeholder: String = chars.by_ref().take_while(|ch| *ch != '}').collect();
            let (index, default) = placeholder
                .split_once(':')
                .expect("placeholder must have a default");
            assert!(index.parse::<u32>().is_ok());
            output.push_str(default);
        } else {
            let index: String =
                std::iter::from_fn(|| chars.next_if(|ch| ch.is_ascii_digit())).collect();
            assert_eq!(index, "0", "only final tabstop may omit a default");
        }
    }
    output
}

#[test]
fn every_snippet_default_expands_into_valid_wit_in_its_context() {
    let snippets: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root().join("snippets/wit.json")).unwrap())
            .unwrap();
    let snippets = snippets.as_object().unwrap();
    assert_eq!(snippets.len(), 15);
    let mut prefixes = BTreeSet::new();
    for (name, snippet) in snippets {
        let prefix = snippet["prefix"].as_str().unwrap();
        assert!(prefixes.insert(prefix), "duplicate snippet prefix {prefix}");
        assert!(!snippet["description"].as_str().unwrap().is_empty());
        let body = snippet["body"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let expanded = expand_defaults(&body);
        let source = match name.as_str() {
            "Package" | "Interface" | "World" => expanded,
            "Import function" | "Export interface" | "Include world" => {
                format!("world app {{ {expanded} }}")
            }
            _ => format!("interface api {{ {expanded} }}"),
        };
        assert!(
            !parse(&source).root_node().has_error(),
            "invalid snippet {name}: {source}"
        );
        println!("valid snippet {prefix}");
    }
}

#[test]
fn extension_and_native_tooling_share_the_exact_grammar_pin() {
    let extension: toml::Value =
        toml::from_str(&fs::read_to_string(root().join("extension.toml")).unwrap()).unwrap();
    let native: toml::Value =
        toml::from_str(&fs::read_to_string(root().join("crates/wit-syntax/Cargo.toml")).unwrap())
            .unwrap();
    let pin = "cdf07263b136054b413cab449ac7a1d059c27542";
    let repository = "https://github.com/bytecodealliance/tree-sitter-wit";
    assert_eq!(extension["grammars"]["wit"]["rev"].as_str(), Some(pin));
    assert_eq!(
        native["dependencies"]["tree-sitter-wit"]["rev"].as_str(),
        Some(pin)
    );
    assert_eq!(
        extension["grammars"]["wit"]["repository"].as_str(),
        Some(repository)
    );
    assert_eq!(
        native["dependencies"]["tree-sitter-wit"]["git"].as_str(),
        Some(repository)
    );
    assert!(
        extension["snippets"]
            .as_array()
            .unwrap()
            .contains(&toml::Value::String("snippets/wit.json".into()))
    );
    println!("Zed and native grammar pin match: {pin} (ABI 15)");
}
