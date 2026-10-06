use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

const MANUAL_SEMANTIC: &str = include_str!("../../../tests/manual-zed/semantic/main.wit");
const MANUAL_ESCAPED: &str = include_str!("../../../tests/manual-zed/escaped/main.wit");
const MANUAL_ESCAPED_ALIAS: &str = include_str!("../../../tests/manual-zed/escaped-alias/main.wit");
const MANUAL_OVERLAY_MAIN: &str = include_str!("../../../tests/manual-zed/overlay/main.wit");
const MANUAL_OVERLAY_TYPES: &str = include_str!("../../../tests/manual-zed/overlay/types.wit");
const MANUAL_DEPENDENCY_MAIN: &str = include_str!("../../../tests/manual-zed/dependency/main.wit");
const MANUAL_DEPENDENCY_TYPES: &str =
    include_str!("../../../tests/manual-zed/dependency/deps/types.wit");
const MANUAL_UNICODE: &str = include_str!("../../../tests/manual-zed/unicode/main.wit");
const MANUAL_FORMATTING_MAIN: &str = include_str!("../../../tests/manual-zed/formatting/main.wit");
const MANUAL_FORMATTING_COMMENTS: &str =
    include_str!("../../../tests/manual-zed/formatting/comments.wit");

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Value>,
}
impl Client {
    fn start(encoding: &str) -> Self {
        Self::start_watching(encoding, false)
    }
    fn start_watching(encoding: &str, watching: bool) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wit-language-server"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut length = None;
                loop {
                    let mut line = String::new();
                    if output.read_line(&mut line).unwrap() == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = Some(value.trim().parse::<usize>().unwrap());
                    }
                }
                let mut body = vec![0; length.unwrap()];
                output.read_exact(&mut body).unwrap();
                if sender.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    return;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            messages,
        };
        client.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{"general":{"positionEncodings":[encoding]},"workspace":{"didChangeWatchedFiles":{"dynamicRegistration":watching}}}}}));
        let result = client.until(|v| v["id"] == 1);
        assert_eq!(
            result["result"]["capabilities"]["positionEncoding"],
            encoding
        );
        assert_eq!(
            result["result"]["serverInfo"]["version"],
            format!(
                "{}+git.{}",
                env!("CARGO_PKG_VERSION"),
                env!("WIT_LANGUAGE_SERVER_BUILD_COMMIT")
            )
        );
        assert_eq!(result["result"]["capabilities"]["hoverProvider"], true);
        assert_eq!(result["result"]["capabilities"]["definitionProvider"], true);
        assert_eq!(result["result"]["capabilities"]["referencesProvider"], true);
        assert_eq!(
            result["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
            json!(["quickfix"])
        );
        assert!(
            result["result"]["capabilities"]
                .get("renameProvider")
                .is_none()
                || result["result"]["capabilities"]["renameProvider"].is_null()
                || result["result"]["capabilities"]["renameProvider"] == false
        );
        assert!(
            result["result"]["capabilities"]
                .get("workspaceSymbolProvider")
                .is_none()
                || result["result"]["capabilities"]["workspaceSymbolProvider"].is_null()
                || result["result"]["capabilities"]["workspaceSymbolProvider"] == false
        );
        client.notify("initialized", json!({}));
        let startup_log = client.until(|v| v["method"] == "window/logMessage");
        assert_eq!(startup_log["params"]["type"], 3);
        assert_eq!(
            startup_log["params"]["message"],
            format!(
                "wit-language-server {}+git.{}",
                env!("CARGO_PKG_VERSION"),
                env!("WIT_LANGUAGE_SERVER_BUILD_COMMIT")
            )
        );
        if watching {
            let registration = client.until(|v| v["method"] == "client/registerCapability");
            assert_eq!(
                registration["params"]["registrations"][0]["method"],
                "workspace/didChangeWatchedFiles"
            );
            assert_eq!(
                registration["params"]["registrations"][0]["registerOptions"]["watchers"][0]["globPattern"],
                "**/*.wit"
            );
            client.send(json!({"jsonrpc":"2.0", "id":registration["id"], "result":null}));
        }
        client
    }
    fn send(&mut self, value: Value) {
        let body = serde_json::to_vec(&value).unwrap();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
    }
    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}));
    }
    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        loop {
            let value = self
                .messages
                .recv_timeout(Duration::from_secs(20))
                .expect("server response deadline");
            if predicate(&value) {
                return value;
            }
        }
    }
    fn diagnostics(&self, uri: &str, version: i32) -> Value {
        self.until(|v| {
            v["method"] == "textDocument/publishDiagnostics"
                && v["params"]["uri"] == uri
                && v["params"]["version"] == version
        })["params"]["diagnostics"]
            .clone()
    }
    fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument":{"uri":uri,"languageId":"wit","version":1,"text":text}}),
        );
    }
    fn change(&mut self, uri: &str, text: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}),
        );
    }
    fn shutdown(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}));
        assert!(self.until(|v| v["id"] == 99).get("result").is_some());
        self.notify("exit", json!(null));
        self.input.take();
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
    }
}

#[test]
fn lifecycle_unicode_versions_formatting_and_close() {
    for encoding in ["utf-8", "utf-16"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let uri = url::Url::from_file_path(&path).unwrap().to_string();
        let mut client = Client::start(encoding);
        let bad = "package test:app; interface api { /*🦀*/ type x = missing; }";
        client.open(&uri, bad);
        let diagnostics = client.diagnostics(&uri, 1);
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        let prefix = bad.split("missing").next().unwrap();
        let expected = if encoding == "utf-8" {
            prefix.len()
        } else {
            prefix.encode_utf16().count()
        };
        assert_eq!(diagnostics[0]["range"]["start"]["character"], expected);
        let good = "package test:app; // retained 🦀\ninterface api{type x=u32;}";
        client.change(&uri, good, 2);
        assert_eq!(client.diagnostics(&uri, 2), json!([]));
        client.change(&uri, bad, 1);
        client.send(json!({"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}}));
        let edits = client.until(|v| v["id"] == 2);
        let formatted = edits["result"][0]["newText"].as_str().unwrap();
        assert!(formatted.contains("retained 🦀"));
        assert!(!formatted.contains("missing"));
        client.change(&uri, formatted, 3);
        assert_eq!(client.diagnostics(&uri, 3), json!([]));
        client.send(json!({"jsonrpc":"2.0","id":3,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}}));
        assert_eq!(client.until(|v| v["id"] == 3)["result"], json!([]));
        client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
        assert_eq!(
            client
                .until(|v| v["method"] == "textDocument/publishDiagnostics"
                    && v["params"]["uri"] == uri)["params"]["diagnostics"],
            json!([])
        );
        client.shutdown();
    }
}

#[test]
fn dependency_overlay_save_watched_files_and_sibling_errors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("deps")).unwrap();
    let main = dir.path().join("main.wit");
    let dep = dir.path().join("deps/types.wit");
    let main_uri = url::Url::from_file_path(&main).unwrap().to_string();
    let dep_uri = url::Url::from_file_path(&dep).unwrap().to_string();
    let source = "package test:app; interface api { use test:types/api.{thing}; }";
    let good = "package test:types; interface api { type thing = u32; }";
    std::fs::write(&main, source).unwrap();
    std::fs::write(&dep, good).unwrap();
    let mut client = Client::start_watching("utf-16", true);
    client.open(&main_uri, source);
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.open(&dep_uri, "package test:types; interface api {}");
    assert!(
        !client
            .diagnostics(&main_uri, 1)
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.change(&dep_uri, good, 2);
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":dep_uri}}),
    );
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    std::fs::write(&dep, "package test:types; interface api {}").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":dep_uri,"type":2}]}),
    );
    assert!(
        !client
            .diagnostics(&main_uri, 1)
            .as_array()
            .unwrap()
            .is_empty()
    );
    std::fs::write(&dep, good).unwrap();
    client.notify(
        "textDocument/didSave",
        json!({"textDocument":{"uri":main_uri}}),
    );
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.shutdown();
}

#[test]
fn cli_help_version_and_reject_unknown_flags() {
    for flag in ["--help", "--version"] {
        let result = Command::new(env!("CARGO_BIN_EXE_wit-language-server"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(result.status.success());
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert!(stdout.contains("wit-language-server"));
        if flag == "--version" {
            assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
            assert!(stdout.contains(env!("WIT_LANGUAGE_SERVER_BUILD_COMMIT")));
        }
    }
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_wit-language-server"))
            .arg("--unknown")
            .output()
            .unwrap()
            .status
            .success()
    );
}

fn request(client: &mut Client, id: u64, method: &str, params: Value) -> Value {
    client.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
    client.until(|value| value["id"] == id)
}

fn position_at(text: &str, byte: usize, encoding: &str) -> Value {
    let prefix = &text[..byte];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let tail = prefix.rsplit('\n').next().unwrap_or("");
    let character = if encoding == "utf-8" {
        tail.len()
    } else {
        tail.encode_utf16().count()
    };
    json!({"line":line,"character":character})
}

fn completion_labels(response: &Value) -> Vec<&str> {
    response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect()
}

#[test]
fn type_completion_is_scope_safe_and_uses_valid_wit_builtins() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("deps")).unwrap();
    let path = dir.path().join("main.wit");
    let dep = dir.path().join("deps/types.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface a { record hidden { value: u32 } } interface b { record local { value: u32 } use a.{hidden as visible}; call: func(value: local); } world app { import demo:dep/api; }";
    std::fs::write(
        dep,
        "package demo:dep; interface api { record secret { value: u32 } }",
    )
    .unwrap();
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let offset = source.find("call: func(value: local").unwrap() + "call: func(value: ".len();
    let response = request(
        &mut client,
        40,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    let labels: Vec<_> = response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    for visible in ["local", "visible", "f32", "f64"] {
        assert!(labels.contains(&visible), "missing {visible}: {labels:?}");
    }
    for hidden in ["hidden", "secret", "float32", "float64"] {
        assert!(!labels.contains(&hidden), "unexpected {hidden}: {labels:?}");
    }
    client.shutdown();
}

#[test]
fn escaped_identifiers_keep_wit_spelling_across_editor_requests() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source =
        "package demo:escaped; interface api { type %type = string; %func: func(%value: %type); }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let offset = source.rfind("%type").unwrap();
    let completion = request(
        &mut client,
        70,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    let labels: Vec<_> = completion["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"%type"), "{labels:?}");
    assert!(!labels.contains(&"type"), "{labels:?}");

    let function = source.find("%func").unwrap();
    let hover = request(
        &mut client,
        71,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":function}}),
    );
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("%func: func(%value: %type);")
    );

    let definition = request(
        &mut client,
        72,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        source.find("%type").unwrap()
    );
    let references = request(
        &mut client,
        73,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset},"context":{"includeDeclaration":true}}),
    );
    assert_eq!(
        references["result"].as_array().unwrap().len(),
        2,
        "{references}"
    );
    client.shutdown();
}

#[test]
fn escaped_import_alias_completion_and_navigation_use_source_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:escaped; interface shared { type %type = string; } interface api { use shared.{%type as %alias}; call: func(value: %alias); }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let use_offset = source.find("%type as").unwrap();
    let definition = request(
        &mut client,
        75,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":use_offset}}),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        source.find("type %type").unwrap() + "type ".len()
    );

    let alias_use = source.rfind("%alias").unwrap();
    let completion = request(
        &mut client,
        76,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":alias_use}}),
    );
    let labels: Vec<_> = completion["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"%alias"), "{labels:?}");
    assert!(!labels.contains(&"alias"), "{labels:?}");
    let hover = request(
        &mut client,
        77,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":alias_use}}),
    );
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("%alias")
    );
    client.shutdown();
}

#[test]
fn completion_does_not_offer_types_in_parameter_name_position() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface api { call: func(first: u32, par) }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert!(!client.diagnostics(&uri, 1).as_array().unwrap().is_empty());
    let offset = source.find(", par").unwrap() + ", par".len();
    let response = request(
        &mut client,
        74,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    let labels: Vec<_> = response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(
        !labels.contains(&"u8") && !labels.contains(&"f32"),
        "{labels:?}"
    );
    assert!(labels.is_empty(), "{labels:?}");
    client.shutdown();
}

#[test]
fn incomplete_type_completion_uses_only_syntax_visible_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source =
        "package demo:app; interface api { record local { value: u32 } call: func(value: lo) }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert!(!client.diagnostics(&uri, 1).as_array().unwrap().is_empty());

    let offset = source.find("value: lo").unwrap() + "value: lo".len();
    let response = request(
        &mut client,
        48,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    let labels: Vec<_> = response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"local"), "{labels:?}");
    assert!(
        labels.contains(&"f32") && labels.contains(&"f64"),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"float32") && !labels.contains(&"float64"),
        "{labels:?}"
    );
    client.shutdown();
}

#[test]
fn incomplete_completion_preserves_scope_and_hides_global_declarations() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("deps")).unwrap();
    let path = dir.path().join("main.wit");
    std::fs::write(
        dir.path().join("deps/types.wit"),
        "package demo:dep; interface api { record secret { value: u32 } leak: func(); }",
    )
    .unwrap();
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface shared { record item { value: u32 } } interface sibling { record hidden { value: u32 } } interface api { record local { value: u32 } use shared.{item as visible}; call: func(value: vis); } world app { import demo:dep/api; }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    let offset = source.find("value: vis").unwrap() + "value: vis".len();
    let response = request(
        &mut client,
        49,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    let labels: Vec<_> = response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    for expected in ["local", "visible", "f32", "f64"] {
        assert!(labels.contains(&expected), "missing {expected}: {labels:?}");
    }
    for hidden in ["item", "hidden", "secret", "leak", "float32", "float64"] {
        assert!(!labels.contains(&hidden), "unexpected {hidden}: {labels:?}");
    }

    let non_type = source.find("interface api {").unwrap() + "interface api {".len();
    let response = request(
        &mut client,
        50,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":non_type}}),
    );
    assert_eq!(response["result"]["items"], json!([]));
    client.shutdown();
}

#[test]
fn typo_fixes_use_only_types_visible_in_the_diagnostic_scope() {
    for (label, source, dependency, expected) in [
        (
            "sibling-only",
            "package demo:app; interface a { record item { value: u32 } } interface b { call: func(value: itme); }",
            None,
            None,
        ),
        (
            "dependency-only",
            "package demo:app; interface b { call: func(value: secrt); } world app { import demo:dep/api; }",
            Some("package demo:dep; interface api { record secret { value: u32 } }"),
            None,
        ),
        (
            "imported-alias",
            "package demo:app; interface shared { enum status { ready } } interface api { use shared.{status as state}; call: func(value: staet); }",
            None,
            Some("Replace with `state`"),
        ),
        (
            "visible-vs-invisible",
            "package demo:app; interface a { record itme { value: u32 } } interface b { record item { value: u32 } call: func(value: itme); }",
            None,
            Some("Replace with `item`"),
        ),
        (
            "equidistant-visible-types",
            "package demo:app; interface api { record cat { value: u32 } record cut { value: u32 } call: func(value: cot); }",
            None,
            None,
        ),
    ] {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().join(label);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.wit");
        let uri = url::Url::from_file_path(&path).unwrap().to_string();
        if let Some(dependency) = dependency {
            std::fs::create_dir(dir.join("deps")).unwrap();
            std::fs::write(dir.join("deps/dep.wit"), dependency).unwrap();
        }
        let mut client = Client::start("utf-16");
        client.open(&uri, source);
        let diagnostics = client.diagnostics(&uri, 1);
        assert_eq!(diagnostics.as_array().unwrap().len(), 1, "{label}");
        let response = request(
            &mut client,
            41,
            "textDocument/codeAction",
            json!({"textDocument":{"uri":uri},"range":diagnostics[0]["range"],"context":{"diagnostics":diagnostics}}),
        );
        match expected {
            Some(title) => assert_eq!(response["result"][0]["title"], title, "{label}"),
            None => assert_eq!(response["result"], json!([]), "{label}: {response}"),
        }
        client.shutdown();
    }
}

#[test]
fn inline_interfaces_aliases_and_world_functions_have_protocol_navigation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface shared { record entry { value: u32 } enum status { ready } resource connection; resource token; } interface api { use shared.{entry, status as state, connection, token}; nested: func(a: list<entry>, b: option<state>, c: borrow<connection>, d: own<token>); } world app { import clock: interface { use shared.{entry}; read: func() -> entry; } import log: func(message: string); export run: func(); }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let state_offset = source.find("option<state>").unwrap() + "option<".len();
    let hover = request(
        &mut client,
        42,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":state_offset}}),
    );
    let hover_text = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(!hover_text.contains("[method]"));
    assert!(!hover_text.contains("[get]"));

    let inline_use = source.find("use shared.{entry}").unwrap() + "use shared.{".len();
    let return_use = source.rfind("entry").unwrap();
    let references = request(
        &mut client,
        43,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":return_use},"context":{"includeDeclaration":true}}),
    );
    let starts: Vec<_> = references["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|location| location["range"]["start"]["character"].as_u64())
        .collect();
    assert!(starts.contains(&(inline_use as u64)), "{references}");
    assert!(starts.contains(&(return_use as u64)), "{references}");

    let borrowed = source.find("borrow<connection>").unwrap() + "borrow<".len();
    let definition = request(
        &mut client,
        51,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":borrowed}}),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        source.find("connection, token};").unwrap()
    );
    let references = request(
        &mut client,
        52,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":borrowed},"context":{"includeDeclaration":true}}),
    );
    assert!(
        references["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|location| { location["range"]["start"]["character"] == borrowed }),
        "{references}"
    );
    let owned = source.find("own<token>").unwrap() + "own<".len();
    let definition = request(
        &mut client,
        60,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":owned}}),
    );
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        source.find("token};").unwrap()
    );
    let references = request(
        &mut client,
        61,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":owned},"context":{"includeDeclaration":true}}),
    );
    assert!(
        references["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|location| { location["range"]["start"]["character"] == owned }),
        "{references}"
    );

    for (id, token, expected) in [
        (44, "log", "import log: func(message: string);"),
        (45, "run", "export run: func();"),
    ] {
        let offset = source.find(token).unwrap();
        let hover = request(
            &mut client,
            id,
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
        );
        assert!(
            hover["result"]["contents"]["value"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{hover}"
        );
    }
    client.shutdown();
}

#[test]
fn aliases_keep_source_and_local_navigation_identities_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface shared { enum status { ready } } interface api { use shared.{status as state}; call: func(value: state); use shared.{status}; other: func(value: status); }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let original_decl = source.find("enum status").unwrap() + "enum ".len();
    let alias_source = source.find("status as state").unwrap();
    let alias_binding = source.find("as state").unwrap() + "as ".len();
    let alias_use = source.find("value: state").unwrap() + "value: ".len();
    let unaliased_binding = source.rfind("use shared.{status}").unwrap() + "use shared.{".len();
    let unaliased_use = source.rfind("value: status").unwrap() + "value: ".len();
    for (id, offset, target) in [
        (53, alias_source, original_decl),
        (54, alias_binding, alias_binding),
        (55, alias_use, alias_binding),
        (56, unaliased_use, unaliased_binding),
    ] {
        let definition = request(
            &mut client,
            id,
            "textDocument/definition",
            json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
        );
        assert_eq!(
            definition["result"]["range"]["start"]["character"], target,
            "{definition}"
        );
    }
    for (id, offset, expected) in [
        (73, alias_source, "status"),
        (74, alias_binding, "state"),
        (75, alias_use, "state"),
        (76, unaliased_binding, "status"),
    ] {
        let hover = request(
            &mut client,
            id,
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
        );
        assert!(
            hover["result"]["contents"]["value"]
                .as_str()
                .unwrap_or_default()
                .contains(expected),
            "{hover}"
        );
    }
    let original_refs = request(
        &mut client,
        57,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":original_decl},"context":{"includeDeclaration":true}}),
    );
    let original_starts: Vec<_> = original_refs["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|location| location["range"]["start"]["character"].as_u64())
        .collect();
    assert!(
        original_starts.contains(&(original_decl as u64)),
        "{original_refs}"
    );
    assert!(
        original_starts.contains(&(alias_source as u64)),
        "{original_refs}"
    );
    assert!(
        !original_starts.contains(&(unaliased_binding as u64)),
        "{original_refs}"
    );
    assert!(
        !original_starts.contains(&(alias_use as u64)),
        "{original_refs}"
    );

    let alias_refs = request(
        &mut client,
        58,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":alias_binding},"context":{"includeDeclaration":true}}),
    );
    let alias_starts: Vec<_> = alias_refs["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|location| location["range"]["start"]["character"].as_u64())
        .collect();
    assert!(
        alias_starts.contains(&(alias_binding as u64)),
        "{alias_refs}"
    );
    assert!(alias_starts.contains(&(alias_use as u64)), "{alias_refs}");
    assert!(
        !alias_starts.contains(&(original_decl as u64)),
        "{alias_refs}"
    );
    let unaliased_refs = request(
        &mut client,
        59,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":unaliased_binding},"context":{"includeDeclaration":true}}),
    );
    let unaliased_starts: Vec<_> = unaliased_refs["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|location| location["range"]["start"]["character"].as_u64())
        .collect();
    assert!(
        unaliased_starts.contains(&(unaliased_binding as u64)),
        "{unaliased_refs}"
    );
    assert!(
        unaliased_starts.contains(&(unaliased_use as u64)),
        "{unaliased_refs}"
    );
    assert!(
        !unaliased_starts.contains(&(original_decl as u64)),
        "{unaliased_refs}"
    );
    client.shutdown();
}

#[test]
fn function_kind_and_composite_hover_use_source_valid_wit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:app; interface api { record item { value: u32 } resource connection; freestanding: func(a: list<u8>, b: option<item>, c: result<item, string>, d: tuple<u8, string>, e: borrow<connection>, f: own<connection>, g: f32, h: f64); async-call: async func(); resource object { constructor(); method: func(); async-method: async func(); static-method: static func(); async-static: static async func(); value: get() -> u32; value: set(v: u32); static-value: static get() -> u32; static-value: static set(v: u32); } }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));
    for (id, token, required) in [
        (
            62,
            "freestanding",
            "freestanding: func(a: list<u8>, b: option<item>, c: result<item, string>, d: tuple<u8, string>, e: borrow<connection>, f: own<connection>, g: f32, h: f64);",
        ),
        (63, "async-call", "async-call: async func();"),
        (64, "method: func", "method: func();"),
        (65, "async-method", "async-method: async func();"),
        (66, "static-method", "static-method: static func();"),
        (67, "async-static", "async-static: static async func();"),
        (68, "constructor", "constructor();"),
        (69, "value: get", "value: get() -> u32;"),
        (70, "value: set", "value: set(v: u32);"),
        (
            71,
            "static-value: static get",
            "static-value: static get() -> u32;",
        ),
        (
            72,
            "static-value: static set",
            "static-value: static set(v: u32);",
        ),
    ] {
        let offset = source.find(token).unwrap();
        let hover = request(
            &mut client,
            id,
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
        );
        let text = hover["result"]["contents"]["value"]
            .as_str()
            .unwrap_or_default();
        assert!(text.contains(required), "{token}: {hover}");
        for resolver_spelling in [
            "[get]",
            "[set]",
            "[method]",
            "[static]",
            "[constructor]resource",
            "float32",
            "float64",
        ] {
            assert!(!text.contains(resolver_spelling), "{token}: {text}");
        }
    }
    client.shutdown();
}

#[test]
fn hover_signatures_preserve_aliases_and_escaped_parameters_across_function_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package demo:escaped; interface shared { enum status { ready } type %type = string; } interface api { use shared.{status as state, %type as %alias}; call: func(value: state, escaped: %alias); %func: func(%value: string); resource r { static-call: static func(%value: string); constructor(%value: string); method: func(%value: string); value: get() -> string; value: set(%value: string); static-value: static get() -> string; static-value: static set(%value: string); } } world app { import %log: func(%value: string); export %run: func(%value: string); }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    for (id, token, signature) in [
        (
            80,
            "call: func",
            "call: func(value: state, escaped: %alias);",
        ),
        (81, "%func", "%func: func(%value: string);"),
        (
            82,
            "static-call",
            "static-call: static func(%value: string);",
        ),
        (83, "constructor", "constructor(%value: string);"),
        (84, "method: func", "method: func(%value: string);"),
        (85, "value: set", "value: set(%value: string);"),
        (
            86,
            "static-value: static set",
            "static-value: static set(%value: string);",
        ),
        (87, "%log", "import %log: func(%value: string);"),
        (88, "%run", "export %run: func(%value: string);"),
    ] {
        let offset = source.find(token).unwrap();
        let hover = request(
            &mut client,
            id,
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
        );
        let contents = hover["result"]["contents"]["value"]
            .as_str()
            .unwrap_or_default();
        assert!(contents.contains(signature), "{token}: {hover}");
    }
    client.shutdown();
}

#[test]
fn navigation_skips_targets_removed_after_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let dep_dir = dir.path().join("deps");
    std::fs::create_dir(&dep_dir).unwrap();
    let path = dir.path().join("main.wit");
    let dep = dep_dir.join("dep.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let dep_uri = url::Url::from_file_path(&dep).unwrap().to_string();
    let source = "package demo:app; world app { import demo:dep/api; }";
    std::fs::write(
        &dep,
        "package demo:dep; interface api { record entry { value: u32 } }",
    )
    .unwrap();
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));
    std::fs::remove_file(&dep).unwrap();

    let offset = source.rfind("api").unwrap();
    let definition = request(
        &mut client,
        46,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset}}),
    );
    assert!(definition["result"].is_null(), "{definition}");
    let references = request(
        &mut client,
        47,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":offset},"context":{"includeDeclaration":true}}),
    );
    assert!(
        references["result"]
            .as_array()
            .unwrap()
            .iter()
            .all(|location| location["uri"] == uri)
    );
    assert!(
        !references["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|location| location["uri"] == dep_uri)
    );
    client.shutdown();
}

#[test]
fn formatting_honors_spaces_and_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let uri = url::Url::from_file_path(dir.path().join("main.wit"))
        .unwrap()
        .to_string();
    let mut client = Client::start("utf-16");
    client.open(&uri, "package test:app; interface api{call:func();}");
    assert_eq!(client.diagnostics(&uri, 1), json!([]));
    for (id, spaces, size, expected) in [(30, true, 2, "\n  call:"), (31, false, 4, "\n\tcall:")] {
        client.send(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":size,"insertSpaces":spaces}}}));
        let response = client.until(|v| v["id"] == id);
        assert!(
            response["result"][0]["newText"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
    }
    client.send(json!({"jsonrpc":"2.0","id":32,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":0,"insertSpaces":true}}}));
    assert_eq!(client.until(|v| v["id"] == 32)["error"]["code"], -32602);
    client.shutdown();
}

#[test]
fn semantic_requests_resolve_types_and_offer_a_safe_typo_fix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let source = "package test:app; interface api { record item { value: u32 } echo: func(value: item) -> item; }";
    let mut client = Client::start("utf-16");
    client.open(&uri, source);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let reference = source.find("value: item").unwrap() + "value: ".len();
    let position = json!({"line":0,"character":reference});
    client.send(json!({"jsonrpc":"2.0","id":10,"method":"textDocument/hover","params":{"textDocument":{"uri":uri},"position":position}}));
    let hover = client.until(|value| value["id"] == 10);
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("record item"),
        "{hover}"
    );
    assert_eq!(hover["result"]["range"]["start"]["character"], reference);

    client.send(json!({"jsonrpc":"2.0","id":11,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":position}}));
    let definition = client.until(|value| value["id"] == 11);
    assert_eq!(definition["result"]["uri"], uri);
    assert_eq!(
        definition["result"]["range"]["start"]["character"],
        source.find("item {").unwrap()
    );

    client.send(json!({"jsonrpc":"2.0","id":12,"method":"textDocument/references","params":{"textDocument":{"uri":uri},"position":position,"context":{"includeDeclaration":true}}}));
    let references = client.until(|value| value["id"] == 12);
    assert_eq!(references["result"].as_array().unwrap().len(), 3);

    let completion = source.find("value: item").unwrap() + "value: ".len();
    client.send(json!({"jsonrpc":"2.0","id":13,"method":"textDocument/completion","params":{"textDocument":{"uri":uri},"position":{"line":0,"character":completion}}}));
    let completions = client.until(|value| value["id"] == 13);
    assert!(
        completions["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "item")
    );
    client.shutdown();

    let mut client = Client::start("utf-16");
    let invalid =
        "package test:app; interface api { record item { value: u32 } call: func(value: itme); }";
    client.open(&uri, invalid);
    let diagnostics = client.diagnostics(&uri, 1);
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    client.send(json!({"jsonrpc":"2.0","id":14,"method":"textDocument/codeAction","params":{"textDocument":{"uri":uri},"range":diagnostics[0]["range"],"context":{"diagnostics":diagnostics}}}));
    let actions = client.until(|value| value["id"] == 14);
    assert_eq!(
        actions["result"][0]["title"], "Replace with `item`",
        "{actions}"
    );
    assert_eq!(
        actions["result"][0]["edit"]["changes"][uri][0]["newText"],
        "item"
    );
    client.shutdown();
}

#[test]
fn manual_semantic_fixture_runs_hover_navigation_completion_and_typo_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    std::fs::write(&path, MANUAL_SEMANTIC).unwrap();
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let mut client = Client::start("utf-16");
    client.open(&uri, MANUAL_SEMANTIC);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));

    let declaration = MANUAL_SEMANTIC.find("record item").unwrap() + "record ".len();
    let type_use = MANUAL_SEMANTIC.find("value: item").unwrap() + "value: ".len();
    let point = position_at(MANUAL_SEMANTIC, type_use, "utf-16");

    let hover = request(
        &mut client,
        200,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":point}),
    );
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("record item"),
        "{hover}"
    );

    let definition = request(
        &mut client,
        201,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":point}),
    );
    assert_eq!(
        definition["result"]["range"]["start"],
        position_at(MANUAL_SEMANTIC, declaration, "utf-16")
    );

    let references = request(
        &mut client,
        202,
        "textDocument/references",
        json!({"textDocument":{"uri":uri},"position":point,"context":{"includeDeclaration":true}}),
    );
    assert_eq!(references["result"].as_array().unwrap().len(), 3);

    let completion = request(
        &mut client,
        203,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":point}),
    );
    let labels = completion_labels(&completion);
    assert!(labels.contains(&"item"), "{labels:?}");
    assert!(labels.contains(&"u32"), "{labels:?}");

    let typo = MANUAL_SEMANTIC.replacen("value: item", "value: itme", 1);
    client.change(&uri, &typo, 2);
    let diagnostics = client.diagnostics(&uri, 2);
    assert_eq!(diagnostics.as_array().unwrap().len(), 1);
    let actions = request(
        &mut client,
        204,
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},"range":diagnostics[0]["range"],"context":{"diagnostics":diagnostics}}),
    );
    assert_eq!(actions["result"].as_array().unwrap().len(), 1);
    assert_eq!(actions["result"][0]["title"], "Replace with `item`");

    let negative = "package manual:semantic; interface api { call: func(first: u32, par) }";
    client.change(&uri, negative, 3);
    let par = negative.find(", par").unwrap() + ", par".len();
    let completion = request(
        &mut client,
        205,
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":position_at(negative, par, "utf-16")}),
    );
    assert!(completion_labels(&completion).is_empty(), "{completion}");
    client.shutdown();
}

#[test]
fn manual_escaped_fixtures_preserve_explicit_identifier_spelling() {
    for (source, file_name) in [
        (MANUAL_ESCAPED, "escaped.wit"),
        (MANUAL_ESCAPED_ALIAS, "escaped-alias.wit"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(file_name);
        std::fs::write(&path, source).unwrap();
        let uri = url::Url::from_file_path(&path).unwrap().to_string();
        let mut client = Client::start("utf-16");
        client.open(&uri, source);
        assert_eq!(client.diagnostics(&uri, 1), json!([]));

        let token = if source.contains("%alias") {
            "%alias"
        } else {
            "%type"
        };
        let use_offset = source.rfind(token).unwrap();
        let point = position_at(source, use_offset, "utf-16");
        let completion = request(
            &mut client,
            210,
            "textDocument/completion",
            json!({"textDocument":{"uri":uri},"position":point}),
        );
        assert!(
            completion_labels(&completion).contains(&token),
            "{completion}"
        );

        let hover = request(
            &mut client,
            211,
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":point}),
        );
        assert!(
            hover["result"]["contents"]["value"]
                .as_str()
                .unwrap()
                .contains(token),
            "{hover}"
        );
        client.shutdown();
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("alias.wit");
    std::fs::write(&path, MANUAL_ESCAPED_ALIAS).unwrap();
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let mut client = Client::start("utf-16");
    client.open(&uri, MANUAL_ESCAPED_ALIAS);
    assert_eq!(client.diagnostics(&uri, 1), json!([]));
    let imported = MANUAL_ESCAPED_ALIAS.find("shared.{%type").unwrap() + "shared.{".len();
    let definition = request(
        &mut client,
        212,
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":position_at(MANUAL_ESCAPED_ALIAS, imported, "utf-16")}),
    );
    assert_eq!(
        definition["result"]["range"]["start"],
        position_at(
            MANUAL_ESCAPED_ALIAS,
            MANUAL_ESCAPED_ALIAS.find("%type").unwrap(),
            "utf-16"
        )
    );
    client.shutdown();
}

#[test]
fn manual_overlay_fixture_runs_unsaved_change_undo_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.wit");
    let types = dir.path().join("types.wit");
    std::fs::write(&main, MANUAL_OVERLAY_MAIN).unwrap();
    std::fs::write(&types, MANUAL_OVERLAY_TYPES).unwrap();
    let main_uri = url::Url::from_file_path(&main).unwrap().to_string();
    let types_uri = url::Url::from_file_path(&types).unwrap().to_string();
    let mut client = Client::start("utf-16");

    client.open(&main_uri, MANUAL_OVERLAY_MAIN);
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.open(&types_uri, MANUAL_OVERLAY_TYPES);
    assert_eq!(client.diagnostics(&types_uri, 1), json!([]));

    let renamed = MANUAL_OVERLAY_TYPES.replace("record item", "record thing");
    client.change(&types_uri, &renamed, 2);
    assert!(
        !client
            .diagnostics(&main_uri, 1)
            .as_array()
            .unwrap()
            .is_empty()
    );

    client.change(&types_uri, MANUAL_OVERLAY_TYPES, 3);
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));

    client.change(&types_uri, &renamed, 4);
    assert!(
        !client
            .diagnostics(&main_uri, 1)
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":types_uri}}),
    );
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));

    client.open(&types_uri, MANUAL_OVERLAY_TYPES);
    assert_eq!(client.diagnostics(&types_uri, 1), json!([]));
    client.shutdown();
}

#[test]
fn manual_dependency_fixture_anchors_broken_dependency_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    std::fs::create_dir(&deps).unwrap();
    let main = dir.path().join("main.wit");
    let types = deps.join("types.wit");
    std::fs::write(&main, MANUAL_DEPENDENCY_MAIN).unwrap();
    std::fs::write(&types, MANUAL_DEPENDENCY_TYPES).unwrap();
    let main_uri = url::Url::from_file_path(&main).unwrap().to_string();
    let types_uri = url::Url::from_file_path(&types).unwrap().to_string();
    let mut client = Client::start("utf-16");

    client.open(&main_uri, MANUAL_DEPENDENCY_MAIN);
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.open(&types_uri, MANUAL_DEPENDENCY_TYPES);
    assert_eq!(client.diagnostics(&types_uri, 1), json!([]));

    let broken = MANUAL_DEPENDENCY_TYPES.replace("type item = u32;", "type item = ;");
    client.change(&types_uri, &broken, 2);
    let diagnostics = client.diagnostics(&types_uri, 2);
    assert!(!diagnostics.as_array().unwrap().is_empty(), "{diagnostics}");
    assert!(
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["source"] == "wit-parser")
    );

    client.change(&types_uri, MANUAL_DEPENDENCY_TYPES, 3);
    assert_eq!(client.diagnostics(&types_uri, 3), json!([]));
    assert_eq!(client.diagnostics(&main_uri, 1), json!([]));
    client.shutdown();
}

#[test]
fn manual_unicode_fixture_keeps_diagnostic_range_aligned() {
    for encoding in ["utf-8", "utf-16"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        std::fs::write(&path, MANUAL_UNICODE).unwrap();
        let uri = url::Url::from_file_path(&path).unwrap().to_string();
        let mut client = Client::start(encoding);
        client.open(&uri, MANUAL_UNICODE);
        assert_eq!(client.diagnostics(&uri, 1), json!([]));

        let broken = MANUAL_UNICODE.replace("type broken = u32;", "type broken = missing;");
        client.change(&uri, &broken, 2);
        let diagnostics = client.diagnostics(&uri, 2);
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        let start = broken.find("missing").unwrap();
        let end = start + "missing".len();
        assert_eq!(
            diagnostics[0]["range"]["start"],
            position_at(&broken, start, encoding)
        );
        assert_eq!(
            diagnostics[0]["range"]["end"],
            position_at(&broken, end, encoding)
        );
        client.shutdown();
    }
}

#[test]
fn manual_formatting_fixtures_are_idempotent_preserve_comments_and_refuse_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.wit");
    let comments_path = dir.path().join("comments.wit");
    std::fs::write(&main_path, MANUAL_FORMATTING_MAIN).unwrap();
    std::fs::write(&comments_path, MANUAL_FORMATTING_COMMENTS).unwrap();

    for (path, source) in [
        (&main_path, MANUAL_FORMATTING_MAIN),
        (&comments_path, MANUAL_FORMATTING_COMMENTS),
    ] {
        let uri = url::Url::from_file_path(path).unwrap().to_string();
        let mut client = Client::start("utf-16");
        client.open(&uri, source);
        assert_eq!(client.diagnostics(&uri, 1), json!([]));

        let first = request(
            &mut client,
            220,
            "textDocument/formatting",
            json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}),
        );
        let formatted = if first["result"]
            .as_array()
            .is_some_and(|edits| edits.is_empty())
        {
            source.to_owned()
        } else {
            first["result"][0]["newText"].as_str().unwrap().to_owned()
        };
        if source == MANUAL_FORMATTING_COMMENTS {
            for retained in [
                "// Formatting must preserve this line comment.",
                "/// Formatting must preserve this doc comment.",
                "@since(version = 1.0.0)",
            ] {
                assert!(formatted.contains(retained), "{formatted}");
            }
        }
        client.change(&uri, &formatted, 2);
        assert_eq!(client.diagnostics(&uri, 2), json!([]));
        let second = request(
            &mut client,
            221,
            "textDocument/formatting",
            json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}),
        );
        assert_eq!(second["result"], json!([]));

        let invalid = formatted.trim_end().strip_suffix('}').unwrap();
        client.change(&uri, invalid, 3);
        assert!(!client.diagnostics(&uri, 3).as_array().unwrap().is_empty());
        let refusal = request(
            &mut client,
            222,
            "textDocument/formatting",
            json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}),
        );
        let explicitly_refused = refusal.get("error").is_some();
        let returned_no_edits = refusal["result"]
            .as_array()
            .is_some_and(|edits| edits.is_empty());
        assert!(
            explicitly_refused || returned_no_edits,
            "invalid formatting must refuse without edits: {refusal}"
        );
        client.shutdown();
    }
}

#[test]
fn manual_parser_diagnostic_clears_after_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let invalid = "package manual:parser; interface api { type broken = ; }";
    let repaired = "package manual:parser; interface api { type broken = u32; }";
    let mut client = Client::start("utf-16");
    client.open(&uri, invalid);
    let diagnostics = client.diagnostics(&uri, 1);
    assert!(!diagnostics.as_array().unwrap().is_empty());
    assert!(
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["source"] == "wit-parser")
    );
    client.change(&uri, repaired, 2);
    assert_eq!(client.diagnostics(&uri, 2), json!([]));
    client.shutdown();
}

#[test]
fn manual_restart_recomputes_diagnostics_in_fresh_server_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.wit");
    let uri = url::Url::from_file_path(&path).unwrap().to_string();
    let invalid = "package manual:restart; interface api { type broken = missing; }";
    std::fs::write(&path, invalid).unwrap();

    let mut messages = Vec::new();
    for _ in 0..2 {
        let mut client = Client::start("utf-16");
        client.open(&uri, invalid);
        let diagnostics = client.diagnostics(&uri, 1);
        assert_eq!(diagnostics.as_array().unwrap().len(), 1);
        messages.push(diagnostics[0]["message"].as_str().unwrap().to_owned());
        client.shutdown();
    }
    assert_eq!(messages[0], messages[1]);
}
