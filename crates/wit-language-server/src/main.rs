//! Native, ordered stdio WIT language server.
use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{Position, Range};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
enum Encoding {
    Utf8,
    Utf16,
}

fn position(text: &str, byte: usize, encoding: Encoding) -> Position {
    let mut boundary = byte.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let prefix = &text[..boundary];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let tail = prefix.rsplit('\n').next().unwrap_or("");
    let character = match encoding {
        Encoding::Utf8 => tail.len(),
        Encoding::Utf16 => tail.encode_utf16().count(),
    };
    Position::new(
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(character).unwrap_or(u32::MAX),
    )
}
fn path(uri: &str) -> Result<PathBuf> {
    url::Url::parse(uri)?
        .to_file_path()
        .map_err(|()| anyhow::anyhow!("only local file URIs are supported"))
}
fn package_directory(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if parent.file_name().is_some_and(|n| n == "deps") {
        return parent.parent();
    }
    if let Some(grandparent) = parent.parent()
        && grandparent.file_name().is_some_and(|n| n == "deps")
    {
        return grandparent.parent();
    }
    Some(parent)
}
fn affects_package(root: &Path, changed: &Path) -> bool {
    let Ok(relative) = changed.strip_prefix(root) else {
        return false;
    };
    let components: Vec<_> = relative.components().collect();
    match components.len() {
        0 => true,
        1 => changed.extension().is_some_and(|e| e == "wit") || relative == Path::new("deps"),
        2 | 3 => {
            components.first().is_some_and(|c| c.as_os_str() == "deps")
                && (changed.extension().is_some_and(|e| e == "wit") || components.len() == 2)
        }
        _ => false,
    }
}
fn uri(path: &Path) -> Result<String> {
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| anyhow::anyhow!("cannot represent file path as URI"))
}
#[derive(Debug)]
struct Document {
    text: String,
    version: i32,
}
struct Server {
    documents: BTreeMap<PathBuf, Document>,
    published: BTreeSet<PathBuf>,
    encoding: Encoding,
    cache: BTreeMap<PathBuf, Result<Vec<wit_analysis::Diagnostic>, String>>,
    watch_registration: bool,
    initialized: bool,
    #[cfg(test)]
    analysis_runs: BTreeMap<PathBuf, usize>,
}

impl Server {
    fn publish(&mut self, connection: &Connection, changed: &[PathBuf]) -> Result<()> {
        let overlays = self
            .documents
            .iter()
            .map(|(p, d)| (p.clone(), d.text.clone()))
            .collect();
        let roots: BTreeSet<_> = self
            .documents
            .keys()
            .filter_map(|p| package_directory(p).map(Path::to_path_buf))
            .collect();
        self.cache.retain(|root, _| roots.contains(root));
        let mut diagnostics: BTreeMap<PathBuf, Vec<Value>> = self
            .documents
            .keys()
            .map(|p| (p.clone(), Vec::new()))
            .collect();
        for root in roots {
            if !self.cache.contains_key(&root) || changed.iter().any(|p| affects_package(&root, p))
            {
                #[cfg(test)]
                {
                    *self.analysis_runs.entry(root.clone()).or_default() += 1;
                }
                self.cache.insert(
                    root.clone(),
                    wit_analysis::analyze(&root, &overlays).map_err(|error| error.to_string()),
                );
            }
            let result = self
                .cache
                .get(&root)
                .context("open root must have cached analysis")?;
            match result {
                Ok(errors) => {
                    for error in errors {
                        // Publish source locations even when the failing sibling is closed.
                        let text = match self.documents.get(&error.path) {
                            Some(doc) => doc.text.clone(),
                            None => std::fs::read_to_string(&error.path).unwrap_or_default(),
                        };
                        let range = Range::new(
                            position(&text, error.range.start, self.encoding),
                            position(&text, error.range.end, self.encoding),
                        );
                        let diagnostic = json!({"range": range, "severity": 1, "source": "wit-parser", "message": error.message});
                        let values = diagnostics.entry(error.path.clone()).or_default();
                        if !values.contains(&diagnostic) {
                            values.push(diagnostic);
                        }
                    }
                }
                Err(error) => {
                    for (p, _) in self
                        .documents
                        .iter()
                        .filter(|(p, _)| package_directory(p) == Some(root.as_path()))
                    {
                        diagnostics.entry(p.clone()).or_default().push(json!({"range": Range::default(), "severity":1, "source":"wit-parser", "message":error.to_string()}));
                    }
                }
            }
        }
        let current: BTreeSet<_> = diagnostics.keys().cloned().collect();
        for stale in self.published.difference(&current) {
            Self::publish_one(connection, stale, None, &[])?;
        }
        for (p, values) in diagnostics {
            Self::publish_one(
                connection,
                &p,
                self.documents.get(&p).map(|d| d.version),
                &values,
            )?;
        }
        self.published = current;
        Ok(())
    }
    fn publish_one(
        connection: &Connection,
        p: &Path,
        version: Option<i32>,
        diagnostics: &[Value],
    ) -> Result<()> {
        connection
            .sender
            .send(Message::Notification(Notification::new(
                "textDocument/publishDiagnostics".into(),
                json!({"uri":uri(p)?, "version":version, "diagnostics":diagnostics}),
            )))?;
        Ok(())
    }
    fn notification(&mut self, connection: &Connection, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            "initialized" => {
                anyhow::ensure!(
                    !self.initialized,
                    "initialized notification received more than once"
                );
                self.initialized = true;
                if self.watch_registration {
                    connection.sender.send(Message::Request(Request::new(
                        "wit-watch-registration".to_owned().into(),
                        "client/registerCapability".into(),
                        json!({"registrations":[{"id":"wit-file-watch","method":"workspace/didChangeWatchedFiles","registerOptions":{"watchers":[{"globPattern":"**/*.wit","kind":7}]}}]}),
                    )))?;
                }
            }
            "textDocument/didOpen" => {
                let params: lsp_types::DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let doc = params.text_document;
                let p = path(doc.uri.as_str())?;
                self.documents.insert(
                    p.clone(),
                    Document {
                        text: doc.text,
                        version: doc.version,
                    },
                );
                self.publish(connection, std::slice::from_ref(&p))?;
            }
            "textDocument/didChange" => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let p = path(params.text_document.uri.as_str())?;
                if let Some(doc) = self.documents.get_mut(&p) {
                    if params.text_document.version <= doc.version {
                        return Ok(());
                    }
                    if params
                        .content_changes
                        .iter()
                        .any(|change| change.range.is_some())
                    {
                        anyhow::bail!("incremental change received despite full-sync negotiation");
                    }
                    if let Some(change) = params.content_changes.into_iter().last() {
                        doc.text = change.text;
                        doc.version = params.text_document.version;
                    }
                }
                self.publish(connection, std::slice::from_ref(&p))?;
            }
            "textDocument/didClose" => {
                let params: lsp_types::DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let p = path(params.text_document.uri.as_str())?;
                self.documents.remove(&p);
                self.publish(connection, std::slice::from_ref(&p))?;
                Self::publish_one(connection, &p, None, &[])?;
                self.published.remove(&p);
            }
            "textDocument/didSave" => {
                let params: lsp_types::DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.publish(connection, &[path(params.text_document.uri.as_str())?])?;
            }
            "workspace/didChangeWatchedFiles" => {
                let params: lsp_types::DidChangeWatchedFilesParams =
                    serde_json::from_value(notification.params)?;
                let paths = params
                    .changes
                    .iter()
                    .map(|event| path(event.uri.as_str()))
                    .collect::<Result<Vec<_>>>()?;
                self.publish(connection, &paths)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn request(&self, request: &Request) -> Result<Value> {
        match request.method.as_str() {
            "textDocument/formatting" => {
                let params: lsp_types::DocumentFormattingParams =
                    serde_json::from_value(request.params.clone())?;
                let p = path(params.text_document.uri.as_str())?;
                let document = self.documents.get(&p).context("document is not open")?;
                let indent = if params.options.insert_spaces {
                    anyhow::ensure!(
                        (1..=16).contains(&params.options.tab_size),
                        "tabSize must be between 1 and 16"
                    );
                    " ".repeat(usize::try_from(params.options.tab_size)?)
                } else {
                    "\t".into()
                };
                let formatted = wit_analysis::format_with_indent(&document.text, &indent)?;
                if formatted == document.text {
                    return Ok(json!([]));
                }
                Ok(
                    json!([{"range":Range::new(Position::new(0,0), position(&document.text, document.text.len(), self.encoding)), "newText":formatted}]),
                )
            }
            _ => anyhow::bail!("unsupported method {}", request.method),
        }
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [arg] if arg == "--version" => {
            println!("wit-language-server {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        [arg] if arg == "--help" || arg == "-h" => {
            println!(
                "wit-language-server\n\nUsage: wit-language-server [--help | --version]\n\nWIT diagnostics and formatting over Language Server Protocol on stdio."
            );
            return Ok(());
        }
        _ => anyhow::bail!("unknown arguments; use --help"),
    }
    let (connection, threads) = Connection::stdio();
    let (initialize_id, initialize) = connection.initialize_start()?;
    let watch_registration = initialize
        .pointer("/capabilities/workspace/didChangeWatchedFiles/dynamicRegistration")
        .and_then(Value::as_bool)
        == Some(true);
    let parameters: lsp_types::InitializeParams = serde_json::from_value(initialize)?;
    let encoding = match parameters
        .capabilities
        .general
        .and_then(|g| g.position_encodings)
    {
        Some(encodings) if encodings.contains(&lsp_types::PositionEncodingKind::UTF8) => {
            Encoding::Utf8
        }
        _ => Encoding::Utf16,
    };
    connection.initialize_finish(
        initialize_id,
        json!({"capabilities": {
        "positionEncoding":match encoding {Encoding::Utf8=>"utf-8",Encoding::Utf16=>"utf-16"},
        "textDocumentSync":{"openClose":true,"change":1,"save":true},
        "documentFormattingProvider":true
    }, "serverInfo":{"name":"wit-language-server","version":env!("CARGO_PKG_VERSION")}}),
    )?;
    let mut server = Server {
        documents: BTreeMap::new(),
        published: BTreeSet::new(),
        encoding,
        cache: BTreeMap::new(),
        watch_registration,
        initialized: false,
        #[cfg(test)]
        analysis_runs: BTreeMap::new(),
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                let response = match server.request(&request) {
                    Ok(value) => Response::new_ok(request.id, value),
                    Err(error) => Response::new_err(
                        request.id,
                        if request.method == "textDocument/formatting" {
                            -32602
                        } else {
                            -32601
                        },
                        error.to_string(),
                    ),
                };
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(notification) => {
                if notification.method == "exit" {
                    anyhow::bail!("exit received before shutdown");
                }
                if let Err(error) = server.notification(&connection, notification) {
                    eprintln!("notification rejected: {error:#}");
                }
            }
            Message::Response(response) => {
                if response.id == lsp_server::RequestId::from("wit-watch-registration".to_owned()) {
                    if let Err(error) = response.response_result {
                        eprintln!(
                            "file-watch registration failed; save refresh remains available: {}",
                            error.message
                        );
                    }
                } else {
                    eprintln!(
                        "ignoring response to unknown server request {}",
                        response.id
                    );
                }
            }
        }
    }
    drop(connection);
    threads.join()?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("wit-language-server: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_cache_reparses_only_affected_open_roots() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let ap = a.join("main.wit");
        let bp = b.join("main.wit");
        let (connection, _client) = Connection::memory();
        let mut server = Server {
            documents: BTreeMap::from([
                (
                    ap.clone(),
                    Document {
                        text: "package test:a; interface api {}".into(),
                        version: 1,
                    },
                ),
                (
                    bp.clone(),
                    Document {
                        text: "package test:b; interface api {}".into(),
                        version: 1,
                    },
                ),
            ]),
            published: BTreeSet::new(),
            encoding: Encoding::Utf16,
            cache: BTreeMap::new(),
            watch_registration: false,
            initialized: false,
            analysis_runs: BTreeMap::new(),
        };
        server.publish(&connection, &[]).unwrap();
        assert_eq!(server.analysis_runs[&a], 1);
        assert_eq!(server.analysis_runs[&b], 1);
        server
            .publish(&connection, std::slice::from_ref(&ap))
            .unwrap();
        assert_eq!(server.analysis_runs[&a], 2);
        assert_eq!(server.analysis_runs[&b], 1);
        server
            .publish(&connection, &[a.join("deps/z/types.wit")])
            .unwrap();
        assert_eq!(server.analysis_runs[&a], 3);
        assert_eq!(server.analysis_runs[&b], 1);
        server
            .publish(
                &connection,
                &[
                    a.join("unrelated/deep/file.wit"),
                    dir.path().join("elsewhere.wit"),
                ],
            )
            .unwrap();
        assert_eq!(server.analysis_runs[&a], 3);
        assert_eq!(server.analysis_runs[&b], 1);
        server.documents.remove(&ap);
        server.publish(&connection, &[ap]).unwrap();
        assert!(!server.cache.contains_key(&a));
        assert!(server.cache.contains_key(&b));
        assert_eq!(server.analysis_runs[&b], 1);
    }
    #[test]
    fn unicode_positions_clamp_inside_codepoints_and_count_surrogates() {
        let text = "a🦀é\r\nx";
        assert_eq!(position(text, 5, Encoding::Utf16), Position::new(0, 3));
        assert_eq!(position(text, 5, Encoding::Utf8), Position::new(0, 5));
        assert_eq!(position(text, 3, Encoding::Utf16), Position::new(0, 1));
        assert_eq!(position(text, 9, Encoding::Utf16), Position::new(1, 0));
        assert_eq!(position(text, 100, Encoding::Utf16), Position::new(1, 1));
    }
}
