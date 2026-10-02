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
        assert!(
            result["result"]["capabilities"]
                .get("hoverProvider")
                .is_none()
        );
        client.notify("initialized", json!({}));
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
