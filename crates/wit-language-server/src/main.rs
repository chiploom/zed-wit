//! Native, ordered stdio WIT language server.
use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{Position, Range};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const BUILD_GIT_COMMIT: &str = env!("WIT_LANGUAGE_SERVER_BUILD_COMMIT");

fn build_version() -> String {
    format!("{}+git.{}", env!("CARGO_PKG_VERSION"), BUILD_GIT_COMMIT)
}

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

fn byte_offset(text: &str, position: Position, encoding: Encoding) -> usize {
    let line_start = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum::<usize>()
        .min(text.len());
    let line_end = text[line_start..]
        .find('\n')
        .map(|offset| line_start + offset)
        .unwrap_or(text.len());
    let line = &text[line_start..line_end];
    let mut units = 0u32;
    let mut bytes = 0usize;
    for ch in line.chars() {
        let width = match encoding {
            Encoding::Utf8 => ch.len_utf8() as u32,
            Encoding::Utf16 => ch.len_utf16() as u32,
        };
        if units + width > position.character {
            break;
        }
        units += width;
        bytes += ch.len_utf8();
    }
    line_start + bytes
}

fn lsp_range(text: &str, range: &std::ops::Range<usize>, encoding: Encoding) -> Range {
    Range::new(
        position(text, range.start, encoding),
        position(text, range.end, encoding),
    )
}

fn key_at(analysis: &wit_analysis::PackageAnalysis, path: &Path, offset: usize) -> Option<String> {
    analysis
        .items
        .iter()
        .find(|item| item.path == path && item.range.start <= offset && offset < item.range.end)
        .map(|item| item.key.clone())
        .or_else(|| {
            analysis
                .references
                .iter()
                .find(|reference| {
                    reference.path == path
                        && reference.range.start <= offset
                        && offset < reference.range.end
                })
                .map(|reference| reference.key.clone())
        })
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, a) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(a != *b));
            diagonal = previous;
        }
    }
    row[right.len()]
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
    cache: BTreeMap<PathBuf, Result<wit_analysis::PackageAnalysis, String>>,
    #[cfg(test)]
    analysis_runs: BTreeMap<PathBuf, usize>,
}

impl Server {
    fn source_text(&self, path: &Path) -> Option<String> {
        self.documents
            .get(path)
            .map(|document| document.text.clone())
            .or_else(|| std::fs::read_to_string(path).ok())
    }

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
                    wit_analysis::analyze_package(&root, &overlays)
                        .map_err(|error| error.to_string()),
                );
            }
            let result = self
                .cache
                .get(&root)
                .context("open root must have cached analysis")?;
            match result {
                Ok(analysis) => {
                    for error in &analysis.diagnostics {
                        // Publish source locations even when the failing sibling is closed.
                        let Some(text) = self.source_text(&error.path) else {
                            continue;
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
            "textDocument/hover"
            | "textDocument/definition"
            | "textDocument/references"
            | "textDocument/completion"
            | "textDocument/codeAction" => {
                let document_uri = request
                    .params
                    .pointer("/textDocument/uri")
                    .and_then(Value::as_str)
                    .context("textDocument.uri is required")?;
                let path = path(document_uri)?;
                let document = self.documents.get(&path).context("document is not open")?;
                let root = package_directory(&path).context("document has no package directory")?;
                let analysis = self
                    .cache
                    .get(root)
                    .and_then(|result| result.as_ref().ok())
                    .context("package analysis is unavailable")?;
                let point = request
                    .params
                    .pointer("/position")
                    .and_then(|value| serde_json::from_value::<Position>(value.clone()).ok());
                match request.method.as_str() {
                    "textDocument/hover" => {
                        let Some(point) = point else {
                            return Ok(Value::Null);
                        };
                        let offset = byte_offset(&document.text, point, self.encoding);
                        let Some(key) = key_at(analysis, &path, offset) else {
                            return Ok(Value::Null);
                        };
                        let Some(item) = analysis.items.iter().find(|item| item.key == key) else {
                            return Ok(Value::Null);
                        };
                        let hover_range = analysis
                            .references
                            .iter()
                            .find(|reference| {
                                reference.key == key
                                    && reference.path == path
                                    && reference.range.start <= offset
                                    && offset < reference.range.end
                            })
                            .map(|reference| reference.range.clone())
                            .unwrap_or_else(|| item.range.clone());
                        let docs = item
                            .documentation
                            .as_deref()
                            .map(|docs| format!("\n\n{docs}"))
                            .unwrap_or_default();
                        let declaration = item
                            .signature
                            .as_deref()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("{} {}", item.kind, item.name));
                        let detail = if item.signature.is_some() || item.detail.is_empty() {
                            String::new()
                        } else {
                            format!("\n\n{}", item.detail)
                        };
                        Ok(
                            json!({"contents":{"kind":"markdown","value":format!("```wit\n{declaration}\n```{detail}{docs}")},"range":lsp_range(&document.text, &hover_range, self.encoding)}),
                        )
                    }
                    "textDocument/definition" => {
                        let Some(point) = point else {
                            return Ok(Value::Null);
                        };
                        let offset = byte_offset(&document.text, point, self.encoding);
                        let Some(key) = key_at(analysis, &path, offset) else {
                            return Ok(Value::Null);
                        };
                        let Some(item) = analysis.items.iter().find(|item| item.key == key) else {
                            return Ok(Value::Null);
                        };
                        let Some(target_text) = self.source_text(&item.path) else {
                            return Ok(Value::Null);
                        };
                        Ok(
                            json!({"uri":uri(&item.path)?,"range":lsp_range(&target_text, &item.range, self.encoding)}),
                        )
                    }
                    "textDocument/references" => {
                        let Some(point) = point else {
                            return Ok(json!([]));
                        };
                        let offset = byte_offset(&document.text, point, self.encoding);
                        let Some(key) = key_at(analysis, &path, offset) else {
                            return Ok(json!([]));
                        };
                        let include_declaration = request
                            .params
                            .pointer("/context/includeDeclaration")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        let mut locations = Vec::new();
                        if include_declaration
                            && let Some(item) = analysis.items.iter().find(|item| item.key == key)
                            && let Some(text) = self.source_text(&item.path)
                        {
                            locations.push(json!({"uri":uri(&item.path)?,"range":lsp_range(&text, &item.range, self.encoding)}));
                        }
                        for reference in analysis
                            .references
                            .iter()
                            .filter(|reference| reference.key == key)
                        {
                            if let Some(text) = self.source_text(&reference.path) {
                                locations.push(json!({"uri":uri(&reference.path)?,"range":lsp_range(&text, &reference.range, self.encoding)}));
                            }
                        }
                        Ok(Value::Array(locations))
                    }
                    "textDocument/completion" => {
                        let Some(point) = point else {
                            return Ok(json!({"isIncomplete":false,"items":[]}));
                        };
                        let offset = byte_offset(&document.text, point, self.encoding);
                        let type_context = wit_analysis::is_type_position(&document.text, offset);
                        let mut items = Vec::new();
                        let primitives = [
                            "bool", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32",
                            "f64", "char", "string",
                        ];
                        if type_context {
                            items.extend(primitives.iter().map(|name| json!({"label":name,"kind":25,"detail":"WIT primitive type"})));
                            items.extend(
                                analysis.visible_types_at(&path, offset).iter().map(
                                    |ty| json!({"label":ty.name,"kind":25,"detail":ty.detail}),
                                ),
                            );
                            if analysis.scopes.is_empty() {
                                items.extend(
                                    wit_analysis::syntax_visible_type_names_at(
                                        &document.text,
                                        offset,
                                    )
                                    .iter()
                                    .map(
                                        |name| json!({"label":name,"kind":25,"detail":"WIT type"}),
                                    ),
                                );
                            }
                        }
                        Ok(json!({"isIncomplete":false,"items":items}))
                    }
                    "textDocument/codeAction" => {
                        let mut actions = Vec::new();
                        for diagnostic in request
                            .params
                            .pointer("/context/diagnostics")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            let message = diagnostic
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if !(message.starts_with("type `") || message.starts_with("name `"))
                                || !message.contains(" does not exist")
                            {
                                continue;
                            }
                            let Some(range) = diagnostic.get("range").cloned() else {
                                continue;
                            };
                            let Some(range) = serde_json::from_value::<Range>(range).ok() else {
                                continue;
                            };
                            let start = byte_offset(&document.text, range.start, self.encoding);
                            let end = byte_offset(&document.text, range.end, self.encoding);
                            if start > end
                                || end > document.text.len()
                                || !document.text.is_char_boundary(start)
                                || !document.text.is_char_boundary(end)
                            {
                                continue;
                            }
                            if !wit_analysis::is_named_type_reference(&document.text, start..end) {
                                continue;
                            }
                            let missing = &document.text[start..end];
                            let mut names: BTreeSet<_> = analysis
                                .visible_types_at(&path, start)
                                .iter()
                                .map(|ty| ty.name.clone())
                                .collect();
                            if analysis.scopes.is_empty() {
                                names.extend(wit_analysis::syntax_visible_type_names_at(
                                    &document.text,
                                    start,
                                ));
                            }
                            let mut candidates: Vec<_> = names
                                .iter()
                                .map(|name| (edit_distance(missing, name), name.as_str()))
                                .collect();
                            candidates.sort();
                            let Some((distance, suggestion)) = candidates.first().copied() else {
                                continue;
                            };
                            if distance > 2
                                || candidates.get(1).is_some_and(|next| next.0 == distance)
                            {
                                continue;
                            }
                            actions.push(json!({"title":format!("Replace with `{suggestion}`"),"kind":"quickfix","diagnostics":[diagnostic],"edit":{"changes":{uri(&path)?:[{"range":range,"newText":suggestion}]}}}));
                        }
                        Ok(Value::Array(actions))
                    }
                    _ => unreachable!(),
                }
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
            println!("wit-language-server {}", build_version());
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
        "documentFormattingProvider":true,
        "hoverProvider":true,
        "completionProvider":{"triggerCharacters":[":","/","{","<",","]},
        "definitionProvider":true,
        "referencesProvider":true,
        "codeActionProvider":{"codeActionKinds":["quickfix"]}
    }, "serverInfo":{"name":"wit-language-server","version":build_version()}}),
    )?;
    connection
        .sender
        .send(Message::Notification(Notification::new(
            "window/logMessage".into(),
            json!({
                "type": 3,
                "message": format!("wit-language-server {}", build_version())
            }),
        )))?;
    if watch_registration {
        connection.sender.send(Message::Request(Request::new("wit-watch-registration".to_owned().into(), "client/registerCapability".into(), json!({"registrations":[{"id":"wit-file-watch","method":"workspace/didChangeWatchedFiles","registerOptions":{"watchers":[{"globPattern":"**/*.wit","kind":7}]}}]}))))?;
    }
    let mut server = Server {
        documents: BTreeMap::new(),
        published: BTreeSet::new(),
        encoding,
        cache: BTreeMap::new(),
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
                        if matches!(
                            request.method.as_str(),
                            "textDocument/formatting"
                                | "textDocument/hover"
                                | "textDocument/definition"
                                | "textDocument/references"
                                | "textDocument/completion"
                                | "textDocument/codeAction"
                        ) {
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
    fn closing_document_preserves_disk_diagnostics_for_closed_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let deps = dir.path().join("deps");
        std::fs::create_dir(&deps).unwrap();
        let main_path = dir.path().join("main.wit");
        let dep_path = deps.join("types.wit");
        let main_source = "package test:app; world app {}";
        let dep_overlay = "package test:types; interface api {}";
        std::fs::write(&main_path, main_source).unwrap();
        std::fs::write(
            &dep_path,
            "package test:types; interface api { type broken = ; }",
        )
        .unwrap();

        let (connection, client) = Connection::memory();
        let mut server = Server {
            documents: BTreeMap::from([
                (
                    main_path.clone(),
                    Document {
                        text: main_source.into(),
                        version: 1,
                    },
                ),
                (
                    dep_path.clone(),
                    Document {
                        text: dep_overlay.into(),
                        version: 1,
                    },
                ),
            ]),
            published: BTreeSet::new(),
            encoding: Encoding::Utf16,
            cache: BTreeMap::new(),
            analysis_runs: BTreeMap::new(),
        };
        server.publish(&connection, &[]).unwrap();
        while client.receiver.try_recv().is_ok() {}

        let dep_uri = uri(&dep_path).unwrap();
        server
            .notification(
                &connection,
                Notification::new(
                    "textDocument/didClose".into(),
                    json!({"textDocument":{"uri":dep_uri}}),
                ),
            )
            .unwrap();

        let mut last = None;
        for message in client.receiver.try_iter() {
            if let Message::Notification(notification) = message
                && notification.method == "textDocument/publishDiagnostics"
                && notification.params["uri"].as_str() == Some(dep_uri.as_str())
            {
                last = Some(notification.params);
            }
        }
        let params = last.expect("closed dependency diagnostics were not published");
        assert!(params["version"].is_null());
        assert!(
            !params["diagnostics"].as_array().unwrap().is_empty(),
            "on-disk errors must remain visible after closing an overlay"
        );
        assert!(server.published.contains(&dep_path));
    }

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
    fn symbol_lookup_uses_half_open_ranges_at_adjacent_boundaries() {
        let path = PathBuf::from("/tmp/main.wit");
        let item = |key: &str, range| wit_analysis::SemanticItem {
            key: key.into(),
            name: key.into(),
            insertion_name: key.into(),
            kind: "record".into(),
            detail: "record type".into(),
            signature: None,
            documentation: None,
            path: path.clone(),
            range,
        };
        let analysis = wit_analysis::PackageAnalysis {
            items: vec![item("first", 2..5), item("second", 5..8)],
            references: vec![wit_analysis::SemanticReference {
                key: "type:2".into(),
                path: path.clone(),
                range: 10..12,
            }],
            ..Default::default()
        };
        assert_eq!(key_at(&analysis, &path, 2).as_deref(), Some("first"));
        assert_eq!(key_at(&analysis, &path, 4).as_deref(), Some("first"));
        assert_eq!(key_at(&analysis, &path, 5).as_deref(), Some("second"));
        assert_eq!(key_at(&analysis, &path, 11).as_deref(), Some("type:2"));
        assert_eq!(key_at(&analysis, &path, 12), None);
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
