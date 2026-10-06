use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

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
            "visible-vs-invisible",
            "package demo:app; interface a { record itme { value: u32 } } interface b { record item { value: u32 } call: func(value: itme); }",
            None,
            Some("Replace with `item`"),
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
    let source = "package demo:app; interface shared { record entry { value: u32 } enum status { ready } resource connection; } interface api { use shared.{entry, status as state, connection}; nested: func(a: list<entry>, b: option<state>, c: borrow<connection>); } world app { import clock: interface { use shared.{entry}; read: func() -> entry; } import log: func(message: string); export run: func(); }";
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
