//! WIT package analysis and concrete-syntax formatting, independent of LSP.
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};
use wit_parser::{Resolve, SourceMap, UnresolvedPackageGroup};

/// A named WIT declaration with its resolved identity and source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticItem {
    pub key: String,
    pub name: String,
    pub insertion_name: String,
    pub kind: String,
    pub detail: String,
    pub signature: Option<String>,
    pub documentation: Option<String>,
    pub path: PathBuf,
    pub range: Range<usize>,
}

/// A type visible at a source location, with its resolved identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleType {
    pub name: String,
    pub normalized_name: String,
    pub key: String,
    pub detail: String,
}

/// A resolved WIT namespace and the source region it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticScope {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub types: Vec<VisibleType>,
}

/// A source occurrence associated with a resolved declaration identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReference {
    pub key: String,
    pub path: PathBuf,
    pub range: Range<usize>,
}

/// Semantic information for one package root, including any diagnostics.
#[derive(Debug, Clone, Default)]
pub struct PackageAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub items: Vec<SemanticItem>,
    pub references: Vec<SemanticReference>,
    pub scopes: Vec<SemanticScope>,
}

impl PackageAnalysis {
    /// Return type names visible at a source position in its resolved WIT scope.
    pub fn visible_types_at(&self, path: &Path, offset: usize) -> &[VisibleType] {
        scope_at(&self.scopes, path, offset)
            .map(|scope| scope.types.as_slice())
            .unwrap_or_default()
    }
}

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

/// Analyze a package and retain its resolved declaration and reference index.
///
/// # Errors
/// Returns an IO error when a discovered source cannot be read.
pub fn analyze_package(directory: &Path, overlays: &Overlays) -> Result<PackageAnalysis, Error> {
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
    let mut analysis = PackageAnalysis::default();
    if diagnostics.is_empty()
        && let Some(main) = main
    {
        let mut resolve = Resolve::default();
        match resolve.push_groups(main, dependencies) {
            Ok(_) => {
                let syntax = parsed_sources(&resolve, overlays);
                analysis.items = semantic_items(&resolve, &syntax);
                analysis.scopes = semantic_scopes(&resolve, &syntax);
                analysis.references = semantic_references(&resolve, &syntax, &analysis.scopes);
            }
            Err(error) => {
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
    }
    analysis.diagnostics = diagnostics;
    Ok(analysis)
}

/// Analyze sibling files and files/package directories immediately below `deps/`.
///
/// # Errors
/// Returns an IO error when a discovered source cannot be read.
pub fn analyze(directory: &Path, overlays: &Overlays) -> Result<Vec<Diagnostic>, Error> {
    Ok(analyze_package(directory, overlays)?.diagnostics)
}

fn type_description(resolve: &Resolve, id: wit_parser::TypeId) -> String {
    match &resolve.types[id].kind {
        wit_parser::TypeDefKind::Type(wit_parser::Type::Id(inner)) => {
            format!("{} type", resolve.types[*inner].kind.as_str())
        }
        kind => format!("{} type", kind.as_str()),
    }
}

struct ParsedSource {
    path: PathBuf,
    source: String,
    tree: tree_sitter::Tree,
}

fn parsed_sources(resolve: &Resolve, overlays: &Overlays) -> Vec<ParsedSource> {
    resolve
        .source_map
        .source_files()
        .filter_map(|path| {
            let source = source_text(path, overlays)?;
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&wit_syntax::language()).ok()?;
            let tree = parser.parse(&source, None)?;
            Some(ParsedSource {
                path: PathBuf::from(path),
                source,
                tree,
            })
        })
        .collect()
}

fn source_text_at(sources: &[ParsedSource], path: &str, range: &Range<usize>) -> Option<String> {
    let parsed = sources
        .iter()
        .find(|source| same_source_path(&source.path.to_string_lossy(), Path::new(path)))?;
    parsed.source.get(range.clone()).map(str::to_owned)
}

fn source_name(
    resolve: &Resolve,
    sources: &[ParsedSource],
    span: wit_parser::Span,
    normalized: &str,
) -> String {
    resolve
        .source_map
        .resolve_span(span)
        .and_then(|location| source_text_at(sources, location.path, &location.range))
        .unwrap_or_else(|| normalized.to_owned())
}

fn normalized_identifier(source: &str) -> &str {
    source.strip_prefix('%').unwrap_or(source)
}

fn visible_type_name(
    resolve: &Resolve,
    sources: &[ParsedSource],
    id: wit_parser::TypeId,
    normalized: &str,
) -> String {
    let ty = &resolve.types[id];
    if let Some(location) = resolve.source_map.resolve_span(ty.span)
        && let Some(alias_range) =
            use_alias_source(resolve, ty.span, Path::new(location.path), sources)
        && let Some(alias) = source_text_at(sources, location.path, &alias_range)
    {
        return alias;
    }
    source_name(resolve, sources, ty.span, normalized)
}

fn use_alias_source(
    resolve: &Resolve,
    span: wit_parser::Span,
    path: &Path,
    sources: &[ParsedSource],
) -> Option<Range<usize>> {
    let location = resolve.source_map.resolve_span(span)?;
    if !same_source_path(location.path, path) {
        return None;
    }
    let source = sources
        .iter()
        .find(|source| same_source_path(&source.path.to_string_lossy(), path))?;
    let mut nodes = vec![source.tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.kind() == "use_names_item" {
            let mut descendants = vec![node];
            while let Some(current) = descendants.pop() {
                if current.kind() == "id" && current.byte_range() == location.range {
                    let alias = child_kind(node, "alias_item")
                        .and_then(|alias_item| alias_item.child_by_field_name("alias"));
                    return Some(alias.map_or(location.range, |alias| alias.byte_range()));
                }
                for index in (0..current.child_count()).rev() {
                    if let Ok(index) = u32::try_from(index)
                        && let Some(child) = current.child(index)
                    {
                        descendants.push(child);
                    }
                }
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    None
}

fn semantic_items(resolve: &Resolve, sources: &[ParsedSource]) -> Vec<SemanticItem> {
    let mut items = Vec::new();
    let mut add = |key: String,
                   name: String,
                   kind: &str,
                   detail: String,
                   signature: Option<String>,
                   documentation: Option<String>,
                   span| {
        if let Some(location) = resolve.source_map.resolve_span(span) {
            let source_name =
                source_text_at(sources, location.path, &location.range).unwrap_or(name);
            items.push(SemanticItem {
                key,
                insertion_name: source_name.clone(),
                name: source_name,
                kind: kind.into(),
                detail,
                signature,
                documentation,
                path: location.path.into(),
                range: location.range,
            });
        }
    };
    for (id, interface) in resolve.interfaces.iter() {
        if let Some(name) = &interface.name {
            add(
                format!("interface:{}", id.index()),
                name.clone(),
                "interface",
                "interface".into(),
                None,
                interface.docs.contents.clone(),
                interface.span,
            );
        }
        for function in interface.functions.values() {
            let name = source_name(resolve, sources, function.span, function.item_name());
            let signature = interface_function_signature(resolve, function, sources);
            let declaration = if matches!(function.kind, wit_parser::FunctionKind::Constructor(_)) {
                format!("{signature};")
            } else {
                format!("{name}: {signature};")
            };
            add(
                format!("function:{}:{}", id.index(), function.name),
                name.clone(),
                "function",
                signature.clone(),
                Some(declaration),
                function.docs.contents.clone(),
                function.span,
            );
        }
    }
    for (id, world) in resolve.worlds.iter() {
        add(
            format!("world:{}", id.index()),
            world.name.clone(),
            "world",
            "world".into(),
            None,
            world.docs.contents.clone(),
            world.span,
        );
        for (direction, entries) in [("import", &world.imports), ("export", &world.exports)] {
            for item in entries.values() {
                let wit_parser::WorldItem::Function(function) = item else {
                    continue;
                };
                let name = source_name(resolve, sources, function.span, function.item_name());
                let signature = interface_function_signature(resolve, function, sources);
                add(
                    format!(
                        "function:world:{}:{direction}:{}",
                        id.index(),
                        function.name
                    ),
                    name.clone(),
                    "function",
                    signature.clone(),
                    Some(format!("{direction} {name}: {signature};")),
                    function.docs.contents.clone(),
                    function.span,
                );
            }
        }
    }
    drop(add);
    for (id, ty) in resolve.types.iter() {
        if let Some(name) = &ty.name {
            let alias_location = resolve
                .source_map
                .resolve_span(ty.span)
                .and_then(|location| {
                    use_alias_source(resolve, ty.span, Path::new(location.path), sources)
                        .map(|range| (PathBuf::from(location.path), range))
                });
            let signature = if alias_location.is_some() {
                None
            } else {
                match &ty.kind {
                    wit_parser::TypeDefKind::Type(inner) => Some(format!(
                        "type {} = {};",
                        source_name(resolve, sources, ty.span, name),
                        display_type(resolve, *inner, sources)
                    )),
                    _ => None,
                }
            };
            if let Some((path, range)) = alias_location {
                let spelling = source_text_at(sources, &path.to_string_lossy(), &range)
                    .unwrap_or_else(|| name.clone());
                items.push(SemanticItem {
                    key: format!("type:{}", id.index()),
                    name: spelling.clone(),
                    insertion_name: spelling,
                    kind: ty.kind.as_str().into(),
                    detail: type_description(resolve, id),
                    signature,
                    documentation: ty.docs.contents.clone(),
                    path,
                    range,
                });
            } else if let Some(location) = resolve.source_map.resolve_span(ty.span) {
                let spelling = source_name(resolve, sources, ty.span, name);
                items.push(SemanticItem {
                    key: format!("type:{}", id.index()),
                    name: spelling.clone(),
                    insertion_name: spelling,
                    kind: ty.kind.as_str().into(),
                    detail: type_description(resolve, id),
                    signature,
                    documentation: ty.docs.contents.clone(),
                    path: location.path.into(),
                    range: location.range,
                });
            }
        }
    }
    items.sort_by(|a, b| (&a.path, a.range.start, &a.key).cmp(&(&b.path, b.range.start, &b.key)));
    items
}

fn source_parameter_names(
    resolve: &Resolve,
    function: &wit_parser::Function,
    sources: &[ParsedSource],
) -> Vec<String> {
    let Some(location) = resolve.source_map.resolve_span(function.span) else {
        return Vec::new();
    };
    let Some(source) = sources
        .iter()
        .find(|source| same_source_path(&source.path.to_string_lossy(), Path::new(location.path)))
    else {
        return Vec::new();
    };
    let Some(name) = source
        .tree
        .root_node()
        .descendant_for_byte_range(location.range.start, location.range.end)
    else {
        return Vec::new();
    };
    let Some(declaration) = std::iter::successors(Some(name), |node| node.parent())
        .find(|node| matches!(node.kind(), "func_item" | "method_item"))
    else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut nodes = vec![declaration];
    while let Some(node) = nodes.pop() {
        if node.kind() == "named_type"
            && let Some(param_name) = node.child_by_field_name("name")
            && let Some(text) = source.source.get(param_name.byte_range())
        {
            names.push(text.to_owned());
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    names
}

fn interface_function_signature(
    resolve: &Resolve,
    function: &wit_parser::Function,
    sources: &[ParsedSource],
) -> String {
    use wit_parser::FunctionKind as Kind;

    let implicit_receiver = usize::from(matches!(
        function.kind,
        Kind::Method(_) | Kind::AsyncMethod(_) | Kind::MethodGetter(_) | Kind::MethodSetter(_)
    ));
    let source_names = source_parameter_names(resolve, function, sources);
    let params = function
        .params
        .iter()
        .enumerate()
        .skip(implicit_receiver)
        .map(|(index, param)| {
            let name = source_names
                .get(index - implicit_receiver)
                .cloned()
                .unwrap_or_else(|| param.name.clone());
            format!("{name}: {}", display_type(resolve, param.ty, sources))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let result = function
        .result
        .map(|ty| format!(" -> {}", display_type(resolve, ty, sources)))
        .unwrap_or_default();
    match function.kind {
        Kind::Getter | Kind::MethodGetter(_) => format!("get(){result}"),
        Kind::Setter | Kind::MethodSetter(_) => format!("set({params}){result}"),
        Kind::StaticGetter(_) => format!("static get(){result}"),
        Kind::StaticSetter(_) => format!("static set({params}){result}"),
        Kind::Constructor(_) => format!("constructor({params})"),
        Kind::Static(_) | Kind::AsyncStatic(_) => format!(
            "static {}func({params}){result}",
            if matches!(function.kind, Kind::AsyncStatic(_)) {
                "async "
            } else {
                ""
            }
        ),
        Kind::Freestanding | Kind::Method(_) | Kind::AsyncMethod(_) | Kind::AsyncFreestanding => {
            format!(
                "{}func({params}){result}",
                if matches!(
                    function.kind,
                    Kind::AsyncFreestanding | Kind::AsyncMethod(_)
                ) {
                    "async "
                } else {
                    ""
                }
            )
        }
    }
}

fn display_type(resolve: &Resolve, ty: wit_parser::Type, sources: &[ParsedSource]) -> String {
    fn render(
        resolve: &Resolve,
        ty: wit_parser::Type,
        sources: &[ParsedSource],
        stack: &mut BTreeSet<usize>,
    ) -> String {
        use wit_parser::{Handle, Type, TypeDefKind};
        match ty {
            Type::Bool => "bool".into(),
            Type::U8 => "u8".into(),
            Type::U16 => "u16".into(),
            Type::U32 => "u32".into(),
            Type::U64 => "u64".into(),
            Type::S8 => "s8".into(),
            Type::S16 => "s16".into(),
            Type::S32 => "s32".into(),
            Type::S64 => "s64".into(),
            Type::F32 => "f32".into(),
            Type::F64 => "f64".into(),
            Type::Char => "char".into(),
            Type::String => "string".into(),
            Type::ErrorContext => "error-context".into(),
            Type::Id(id) => {
                let definition = &resolve.types[id];
                if let Some(name) = &definition.name {
                    return source_name(resolve, sources, definition.span, name);
                }
                if !stack.insert(id.index()) {
                    return "_".into();
                }
                let mut nested = |ty| render(resolve, ty, sources, stack);
                let rendered = match &definition.kind {
                    TypeDefKind::Handle(Handle::Own(resource)) => {
                        format!("own<{}>", nested(Type::Id(*resource)))
                    }
                    TypeDefKind::Handle(Handle::Borrow(resource)) => {
                        format!("borrow<{}>", nested(Type::Id(*resource)))
                    }
                    TypeDefKind::Tuple(tuple) => format!(
                        "tuple<{}>",
                        tuple
                            .types
                            .iter()
                            .map(|ty| nested(*ty))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    TypeDefKind::Option(inner) => format!("option<{}>", nested(*inner)),
                    TypeDefKind::Result(result) => match (result.ok, result.err) {
                        (None, None) => "result".into(),
                        (Some(ok), None) => format!("result<{}>", nested(ok)),
                        (None, Some(err)) => format!("result<_, {}>", nested(err)),
                        (Some(ok), Some(err)) => format!("result<{}, {}>", nested(ok), nested(err)),
                    },
                    TypeDefKind::List(inner) => format!("list<{}>", nested(*inner)),
                    TypeDefKind::Map(key, value) => {
                        format!("map<{}, {}>", nested(*key), nested(*value))
                    }
                    TypeDefKind::FixedLengthList(inner, length) => {
                        format!("list<{}, {length}>", nested(*inner))
                    }
                    TypeDefKind::Future(inner) => format!(
                        "future{}",
                        inner
                            .map(|ty| format!("<{}>", nested(ty)))
                            .unwrap_or_default()
                    ),
                    TypeDefKind::Stream(inner) => format!(
                        "stream{}",
                        inner
                            .map(|ty| format!("<{}>", nested(ty)))
                            .unwrap_or_default()
                    ),
                    TypeDefKind::Type(inner) => nested(*inner),
                    other => other.as_str().to_owned(),
                };
                stack.remove(&id.index());
                rendered
            }
        }
    }
    render(resolve, ty, sources, &mut BTreeSet::new())
}

fn child_kind<'tree>(
    node: tree_sitter::Node<'tree>,
    kind: &str,
) -> Option<tree_sitter::Node<'tree>> {
    (0..node.child_count())
        .filter_map(|index| u32::try_from(index).ok().and_then(|i| node.child(i)))
        .find(|child| child.kind() == kind)
}

fn interface_scope(
    resolve: &Resolve,
    id: wit_parser::InterfaceId,
    path: &Path,
    range: Range<usize>,
    sources: &[ParsedSource],
) -> SemanticScope {
    SemanticScope {
        path: path.to_path_buf(),
        range,
        types: resolve.interfaces[id]
            .types
            .iter()
            .map(|(name, id)| VisibleType {
                name: visible_type_name(resolve, sources, *id, name),
                normalized_name: name.clone(),
                key: format!("type:{}", id.index()),
                detail: type_description(resolve, *id),
            })
            .collect(),
    }
}

fn world_scope(
    resolve: &Resolve,
    id: wit_parser::WorldId,
    path: &Path,
    range: Range<usize>,
    sources: &[ParsedSource],
) -> SemanticScope {
    let world = &resolve.worlds[id];
    SemanticScope {
        path: path.to_path_buf(),
        range,
        types: world
            .imports
            .values()
            .chain(world.exports.values())
            .filter_map(|item| match item {
                wit_parser::WorldItem::Type { id, .. } => {
                    resolve.types[*id].name.as_ref().map(|name| VisibleType {
                        name: visible_type_name(resolve, sources, *id, name),
                        normalized_name: name.clone(),
                        key: format!("type:{}", id.index()),
                        detail: type_description(resolve, *id),
                    })
                }
                _ => None,
            })
            .collect(),
    }
}

fn same_source_path(source_map_path: &str, path: &Path) -> bool {
    fn normalized(path: &Path) -> PathBuf {
        if let Ok(path) = path.canonicalize() {
            return path;
        }
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => parent
                .canonicalize()
                .map(|parent| parent.join(name))
                .unwrap_or_else(|_| path.to_path_buf()),
            _ => path.to_path_buf(),
        }
    }
    normalized(Path::new(source_map_path)) == normalized(path)
}

fn source_text(path: &Path, overlays: &Overlays) -> Option<String> {
    overlays
        .get(path)
        .or_else(|| {
            overlays
                .iter()
                .find(|(overlay, _)| same_source_path(&overlay.to_string_lossy(), path))
                .map(|(_, text)| text)
        })
        .cloned()
        .or_else(|| std::fs::read_to_string(path).ok())
}

fn semantic_scopes(resolve: &Resolve, sources: &[ParsedSource]) -> Vec<SemanticScope> {
    let mut scopes = Vec::new();
    for source in sources {
        let path = source.path.as_path();
        let mut nodes = vec![source.tree.root_node()];
        while let Some(node) = nodes.pop() {
            match node.kind() {
                "interface_item" => {
                    if let (Some(name_node), Some(body)) =
                        (node.child_by_field_name("name"), child_kind(node, "body"))
                    {
                        for (id, interface) in resolve.interfaces.iter() {
                            if resolve.source_map.resolve_span(interface.span).is_some_and(
                                |location| {
                                    same_source_path(location.path, path)
                                        && location.range == name_node.byte_range()
                                },
                            ) {
                                scopes.push(interface_scope(
                                    resolve,
                                    id,
                                    path,
                                    body.byte_range(),
                                    sources,
                                ));
                                break;
                            }
                        }
                    }
                }
                "world_item" => {
                    if let (Some(name_node), Some(body)) =
                        (node.child_by_field_name("name"), child_kind(node, "body"))
                    {
                        for (id, world) in resolve.worlds.iter() {
                            if resolve
                                .source_map
                                .resolve_span(world.span)
                                .is_some_and(|location| {
                                    same_source_path(location.path, path)
                                        && location.range == name_node.byte_range()
                                })
                            {
                                scopes.push(world_scope(
                                    resolve,
                                    id,
                                    path,
                                    body.byte_range(),
                                    sources,
                                ));
                                break;
                            }
                        }
                    }
                }
                "import_item" | "export_item" => {
                    let name_node = node.child_by_field_name("name");
                    let body = (0..node.child_count())
                        .filter_map(|index| u32::try_from(index).ok().and_then(|i| node.child(i)))
                        .find(|child| child.kind() == "extern_type")
                        .and_then(|extern_type| {
                            (0..extern_type.child_count())
                                .filter_map(|index| {
                                    u32::try_from(index).ok().and_then(|i| extern_type.child(i))
                                })
                                .find(|child| child.kind() == "body")
                        });
                    if let (Some(name_node), Some(body)) = (name_node, body) {
                        for (id, interface) in resolve.interfaces.iter() {
                            if interface.name.is_none()
                                && resolve.source_map.resolve_span(interface.span).is_some_and(
                                    |location| {
                                        same_source_path(location.path, path)
                                            && location.range == name_node.byte_range()
                                    },
                                )
                            {
                                scopes.push(interface_scope(
                                    resolve,
                                    id,
                                    path,
                                    body.byte_range(),
                                    sources,
                                ));
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
            for index in (0..node.child_count()).rev() {
                if let Ok(index) = u32::try_from(index)
                    && let Some(child) = node.child(index)
                {
                    nodes.push(child);
                }
            }
        }
    }
    scopes.sort_by(|a, b| {
        (&a.path, a.range.start, a.range.end).cmp(&(&b.path, b.range.start, b.range.end))
    });
    scopes
}

fn scope_at<'a>(
    scopes: &'a [SemanticScope],
    path: &Path,
    offset: usize,
) -> Option<&'a SemanticScope> {
    scopes
        .iter()
        .filter(|scope| {
            same_source_path(&scope.path.to_string_lossy(), path)
                && scope.range.start <= offset
                && offset < scope.range.end
        })
        .min_by_key(|scope| scope.range.end - scope.range.start)
}

fn semantic_references(
    resolve: &Resolve,
    sources: &[ParsedSource],
    scopes: &[SemanticScope],
) -> Vec<SemanticReference> {
    let mut references = Vec::new();
    for (_, world) in resolve.worlds.iter() {
        for item in world.imports.values().chain(world.exports.values()) {
            if let wit_parser::WorldItem::Interface { id, span, .. } = item
                && let Some(location) = resolve.source_map.resolve_span(*span)
            {
                references.push(SemanticReference {
                    key: format!("interface:{}", id.index()),
                    path: location.path.into(),
                    range: location.range,
                });
            }
        }
        for include in &world.includes {
            if let Some(location) = resolve.source_map.resolve_span(include.span) {
                references.push(SemanticReference {
                    key: format!("world:{}", include.id.index()),
                    path: location.path.into(),
                    range: location.range,
                });
            }
        }
    }
    for parsed in sources {
        let path = parsed.path.as_path();
        let source = &parsed.source;
        let mut nodes = vec![parsed.tree.root_node()];
        while let Some(node) = nodes.pop() {
            for index in (0..node.child_count()).rev() {
                if let Ok(index) = u32::try_from(index)
                    && let Some(child) = node.child(index)
                {
                    nodes.push(child);
                }
            }
            if matches!(node.kind(), "ty" | "handle") {
                let mut descendants = vec![node];
                while let Some(current) = descendants.pop() {
                    if current.kind() == "id" {
                        let start = current.start_byte();
                        if let Some(name) = source.get(current.byte_range())
                            && let Some(scope) = scope_at(scopes, path, start)
                            && let Some(visible) = scope
                                .types
                                .iter()
                                .find(|ty| ty.normalized_name == normalized_identifier(name))
                        {
                            references.push(SemanticReference {
                                key: visible.key.clone(),
                                path: path.to_path_buf(),
                                range: current.byte_range(),
                            });
                        }
                    }
                    for index in (0..current.child_count()).rev() {
                        if let Ok(index) = u32::try_from(index)
                            && let Some(child) = current.child(index)
                        {
                            descendants.push(child);
                        }
                    }
                }
            }
            if node.kind() == "use_names_item" {
                let mut identifiers = Vec::new();
                let mut descendants = vec![node];
                while let Some(current) = descendants.pop() {
                    if current.kind() == "id" {
                        identifiers.push(current);
                    }
                    for index in (0..current.child_count()).rev() {
                        if let Ok(index) = u32::try_from(index)
                            && let Some(child) = current.child(index)
                        {
                            descendants.push(child);
                        }
                    }
                }
                if let Some(scope) = scope_at(scopes, path, node.start_byte()) {
                    for identifier in identifiers {
                        let range = identifier.byte_range();
                        let Some(name) = source.get(range.clone()) else {
                            continue;
                        };
                        let explicit_import_name = child_kind(node, "alias_item")
                            .and_then(|alias| alias.child_by_field_name("alias"))
                            .is_some_and(|alias| alias.byte_range() != range);
                        let original_key = explicit_import_name
                            .then(|| {
                                resolve.types.iter().find_map(|(_, ty)| {
                                    let location = resolve.source_map.resolve_span(ty.span)?;
                                    if !same_source_path(location.path, path)
                                        || location.range != range
                                    {
                                        return None;
                                    }
                                    match &ty.kind {
                                        wit_parser::TypeDefKind::Type(wit_parser::Type::Id(
                                            original,
                                        )) => Some(format!("type:{}", original.index())),
                                        _ => None,
                                    }
                                })
                            })
                            .flatten();
                        let key = original_key.or_else(|| {
                            scope
                                .types
                                .iter()
                                .find(|ty| ty.normalized_name == normalized_identifier(name))
                                .map(|ty| ty.key.clone())
                        });
                        if let Some(key) = key {
                            references.push(SemanticReference {
                                key,
                                path: path.to_path_buf(),
                                range,
                            });
                        }
                    }
                }
            }
        }
    }
    references
        .sort_by(|a, b| (&a.path, a.range.start, &a.key).cmp(&(&b.path, b.range.start, &b.key)));
    references.dedup();
    references
}

/// Return type declaration names even when the complete package does not resolve.
pub fn declared_type_names(source: &str) -> Vec<String> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&wit_syntax::language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        let field = match node.kind() {
            "type_item" => "alias",
            "record_item" | "flags_items" | "enum_items" | "variant_items" | "resource_item" => {
                "name"
            }
            _ => "",
        };
        if !field.is_empty()
            && let Some(name) = node.child_by_field_name(field)
            && let Some(value) = source.get(name.byte_range())
        {
            names.insert(value.to_owned());
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    names.into_iter().collect()
}

/// Return type bindings introduced in the innermost syntax scope at an offset.
/// This remains useful while upstream semantic resolution is blocked by an edit.
pub fn syntax_visible_type_names_at(source: &str, offset: usize) -> Vec<String> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&wit_syntax::language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut bodies = Vec::new();
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        let body = match node.kind() {
            "interface_item" | "world_item" => child_kind(node, "body"),
            "import_item" | "export_item" => child_kind(node, "extern_type")
                .and_then(|extern_type| child_kind(extern_type, "body")),
            _ => None,
        };
        if let Some(body) = body
            && body.start_byte() <= offset
            && offset <= body.end_byte()
        {
            bodies.push(body);
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    let Some(body) = bodies
        .into_iter()
        .min_by_key(|body| body.end_byte() - body.start_byte())
    else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for index in 0..body.child_count() {
        let Some(child) = u32::try_from(index).ok().and_then(|i| body.child(i)) else {
            continue;
        };
        let field = match child.kind() {
            "type_item" => "alias",
            "record_item" | "flags_items" | "enum_items" | "variant_items" | "resource_item" => {
                "name"
            }
            _ => "",
        };
        if !field.is_empty()
            && let Some(name) = child.child_by_field_name(field)
            && let Some(value) = source.get(name.byte_range())
        {
            names.insert(value.to_owned());
        }
        if child.kind() == "use_item" {
            let mut use_nodes = vec![child];
            while let Some(use_node) = use_nodes.pop() {
                if use_node.kind() == "use_names_item" {
                    let alias = child_kind(use_node, "alias_item")
                        .and_then(|item| item.child_by_field_name("alias"));
                    let ids = (0..use_node.child_count())
                        .filter_map(|i| u32::try_from(i).ok().and_then(|i| use_node.child(i)))
                        .filter(|node| node.kind() == "id")
                        .collect::<Vec<_>>();
                    let binding = alias.or_else(|| ids.last().copied());
                    if let Some(binding) = binding
                        && let Some(value) = source.get(binding.byte_range())
                    {
                        names.insert(value.to_owned());
                    }
                }
                for i in (0..use_node.child_count()).rev() {
                    if let Some(child) = u32::try_from(i).ok().and_then(|i| use_node.child(i)) {
                        use_nodes.push(child);
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Return whether the cursor is in a Tree-sitter type or handle node.
pub fn is_type_position(source: &str, offset: usize) -> bool {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&wit_syntax::language()).is_err() {
        return false;
    }
    let Some(tree) = parser.parse(source, None) else {
        return false;
    };
    let root = tree.root_node();
    let probe = offset.min(source.len());
    let node = root.descendant_for_byte_range(probe.saturating_sub(1), probe);
    let mut current = node;
    let mut inside_non_code = false;
    while let Some(node) = current {
        if matches!(node.kind(), "ty" | "handle")
            && node.start_byte() <= probe
            && probe <= node.end_byte()
        {
            return true;
        }
        inside_non_code |= node.kind().contains("comment") || node.kind().contains("string");
        current = node.parent();
    }
    if inside_non_code {
        return false;
    }
    // In this grammar, partial own/borrow arguments can recover as an ERROR
    // after the `ty` node for the handle name; retain that CST context.
    let mut nodes = vec![root];
    while let Some(candidate) = nodes.pop() {
        if candidate.kind() == "named_type"
            && candidate.start_byte() <= probe
            && let Some(ty) = candidate.child_by_field_name("type")
            && source
                .get(ty.byte_range())
                .is_some_and(|text| matches!(text, "own" | "borrow"))
            && source.get(ty.end_byte()..probe).is_some_and(|tail| {
                tail.strip_prefix('<').is_some_and(|argument| {
                    argument.trim().bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'%')
                    })
                })
            })
        {
            return true;
        }
        for index in (0..candidate.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = candidate.child(index)
            {
                nodes.push(child);
            }
        }
    }
    let is_identifier_byte =
        |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'%');
    let bytes = source.as_bytes();
    let mut start = probe;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = probe;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    let prefix = source[..start].trim_end();
    let probe_token = if prefix.ends_with("own<") || prefix.ends_with("borrow<") {
        "r"
    } else {
        "u8"
    };
    let mut probe_source = String::with_capacity(source.len() + probe_token.len());
    probe_source.push_str(&source[..start]);
    probe_source.push_str(probe_token);
    probe_source.push_str(&source[end..]);
    let Some(probe_tree) = parser.parse(&probe_source, None) else {
        return false;
    };
    let candidate = start..start + probe_token.len();

    let mut nodes = vec![probe_tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.byte_range() == candidate {
            let mut parent = Some(node);
            while let Some(parent_node) = parent {
                if matches!(parent_node.kind(), "ty" | "handle") {
                    return true;
                }
                parent = parent_node.parent();
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    false
}

/// Backwards-compatible name for syntax-visible type bindings.
pub fn declared_type_names_at(source: &str, offset: usize) -> Vec<String> {
    syntax_visible_type_names_at(source, offset)
}

/// Return whether the given byte range is a named type in the syntax tree.
pub fn is_named_type_reference(source: &str, range: Range<usize>) -> bool {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&wit_syntax::language()).is_err() {
        return false;
    }
    let Some(tree) = parser.parse(source, None) else {
        return false;
    };
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.kind() == "id" && node.byte_range() == range {
            let mut ancestor = node.parent();
            while let Some(parent) = ancestor {
                if matches!(parent.kind(), "ty" | "handle") {
                    return true;
                }
                ancestor = parent.parent();
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Ok(index) = u32::try_from(index)
                && let Some(child) = node.child(index)
            {
                nodes.push(child);
            }
        }
    }
    false
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
    fn semantic_index_tracks_named_type_uses() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.wit");
        let source = "package test:app; interface api { record item { value: u32 } echo: func(value: item) -> item; }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(main.clone(), source.into())])).unwrap();
        assert!(!analysis.scopes.is_empty());
        assert!(
            analysis
                .references
                .iter()
                .any(|reference| reference.key.starts_with("type:"))
        );
        let type_use = source.find("value: item").unwrap() + "value: ".len();
        assert!(is_named_type_reference(source, type_use..type_use + 4));
        let function_name = source.find("echo").unwrap();
        assert!(!is_named_type_reference(
            source,
            function_name..function_name + 4
        ));
    }
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
    fn formatter_preserves_blank_lines_before_annotation_gates() {
        let source = include_str!("../../../tests/fixtures/current/annotations/annotations.wit");
        assert_eq!(format(source).unwrap(), source);
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
    fn formatter_matches_upstream_use_and_include_spacing() {
        let source = include_str!("../../../tests/fixtures/current/core/core.wit");
        let formatted = format(source).unwrap();

        assert!(formatted.contains("\n}\n\nuse types as shared;\ninterface api"));
        assert!(formatted.contains("include base with { base-log as model-log }"));
        assert!(formatted.contains("use shared.{entry, status as state};"));
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
    fn resolved_scopes_expose_only_local_and_imported_type_spellings() {
        let dir = tempfile::tempdir().unwrap();
        let source = "package test:app; interface a { record hidden { value: u32 } } interface b { record local { value: u32 } use a.{hidden as visible}; call: func(value: visible); }";
        let path = dir.path().join("main.wit");
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path.clone(), source.into())])).unwrap();
        assert!(analysis.diagnostics.is_empty());
        let offset = source.find("value: visible").unwrap();
        let offset = offset + "value: ".len();
        let names: BTreeSet<_> = analysis
            .visible_types_at(&path, offset)
            .iter()
            .map(|ty| ty.name.as_str())
            .collect();
        assert!(names.contains("local"));
        assert!(names.contains("visible"));
        assert!(!names.contains("hidden"));
    }

    #[test]
    fn escaped_identifiers_keep_source_spelling_in_semantic_items_and_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let source = "package demo:escaped; interface api { type %type = string; %func: func(%value: %type); }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path.clone(), source.into())])).unwrap();
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let escaped_type = analysis
            .items
            .iter()
            .find(|item| item.key.starts_with("type:"))
            .unwrap();

        assert_eq!(escaped_type.name, "%type");
        assert_eq!(escaped_type.insertion_name, "%type");
        assert_eq!(
            escaped_type.signature.as_deref(),
            Some("type %type = string;")
        );
        let function = analysis
            .items
            .iter()
            .find(|item| item.kind == "function")
            .unwrap();
        assert_eq!(function.name, "%func");
        assert_eq!(
            function.signature.as_deref(),
            Some("%func: func(%value: %type);")
        );
        assert!(analysis.references.iter().any(|reference| {
            reference.key == escaped_type.key && &source[reference.range.clone()] == "%type"
        }));
        let offset = source.find("value: %type").unwrap() + "value: ".len();
        assert!(
            analysis
                .visible_types_at(&path, offset)
                .iter()
                .any(|ty| ty.name == "%type" && ty.normalized_name == "type")
        );
    }

    #[test]
    fn escaped_type_imports_keep_spelling_and_resolved_references() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let source = "package demo:escaped; interface shared { type %type = string; } interface api { use shared.{%type}; call: func(value: %type); }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path.clone(), source.into())])).unwrap();
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let imported_offset = source.find("use shared.{%type}").unwrap() + "use shared.{".len();
        let imported = analysis
            .items
            .iter()
            .find(|item| item.range.start == imported_offset)
            .unwrap();
        assert_eq!(imported.name, "%type");
        assert!(analysis.references.iter().any(|reference| {
            reference.key == imported.key && &source[reference.range.clone()] == "%type"
        }));
        let use_offset = source.rfind("%type").unwrap();
        assert!(
            analysis
                .visible_types_at(&path, use_offset)
                .iter()
                .any(|ty| {
                    ty.name == "%type" && ty.normalized_name == "type" && ty.key == imported.key
                })
        );
    }

    #[test]
    fn type_context_rejects_partial_non_type_identifiers() {
        for source in [
            "interface api { call: func(first: u32, par|) }",
            "interface api { record x { first: u32, fie| } }",
            "interface api { variant x { first, cas| } }",
            "interface api { enum x { first, cas| } }",
            "interface api { call: fu| }",
            "interface api { resource x { met| } }",
            "interface api { call: func(a: u32, par|) }",
            "world app { import imp| }",
        ] {
            let offset = source.find('|').unwrap();
            let source = source.replace('|', "");
            assert!(!is_type_position(&source, offset), "{source} at {offset}");
        }
    }

    #[test]
    fn type_context_recognizes_partial_names_and_composite_arguments() {
        for source in [
            "interface api { call: func(value: |) }",
            "interface api { call: func(value: lo|) }",
            "interface api { call: func() -> | }",
            "interface api { call: func() -> lo| }",
            "interface api { type x = | }",
            "interface api { type x = list<|> }",
            "interface api { type x = list<lo|> }",
            "interface api { type x = option<lo|> }",
            "interface api { type x = result<lo|, string> }",
            "interface api { type x = result<string, lo|> }",
            "interface api { type x = tuple<lo|, string> }",
            "interface api { type x = tuple<u8, lo|> }",
            "interface api { type x = map<string, lo|> }",
            "interface api { variant outcome { item(lo|) } }",
            "interface api { resource r; type x = borrow<lo|> }",
            "interface api { resource r; call: func(value: own<lo|>) }",
            "interface api { type x = map<lo|, u8> }",
            "interface api { type x = future<lo|> }",
            "interface api { type x = stream<lo|> }",
            "interface api { type x = list<lo|, 4> }",
        ] {
            let offset = source.find('|').unwrap();
            let source = source.replace('|', "");
            assert!(is_type_position(&source, offset), "{source} at {offset}");
        }
    }

    #[test]
    fn function_names_and_composite_signatures_use_wit_source_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let source = "package test:app; interface api { record item { value: u32 } resource connection; resource token; render: func(a: list<u8>, b: option<item>, c: result<item, string>, d: result<_, string>, e: tuple<u8, string>, f: borrow<connection>, g: own<token>, h: future<item>, i: stream<u8>, j: f32, k: f64) -> map<string, list<option<item>>>; async-call: async func(); resource object { constructor(); method: func(); async-method: async func(); static-method: static func(); async-static: static async func(); value: get() -> u32; value: set(v: u32); static-value: static get() -> u32; static-value: static set(v: u32); } }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path, source.into())])).unwrap();
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let render = analysis
            .items
            .iter()
            .find(|item| item.name == "render")
            .unwrap();
        assert_eq!(
            render.detail,
            "func(a: list<u8>, b: option<item>, c: result<item, string>, d: result<_, string>, e: tuple<u8, string>, f: borrow<connection>, g: own<token>, h: future<item>, i: stream<u8>, j: f32, k: f64) -> map<string, list<option<item>>>"
        );
        let functions: Vec<_> = analysis
            .items
            .iter()
            .filter(|item| item.kind == "function")
            .collect();
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("async-method: async func();")),
            "{functions:#?}"
        );
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("static-method: static func();"))
        );
        assert!(functions.iter().any(|item| item.signature.as_deref() == Some("async-static: static async func();")));
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("constructor();"))
        );
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("value: get() -> u32;"))
        );
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("value: set(v: u32);"))
        );
        assert!(functions.iter().any(|item| item.signature.as_deref() == Some("static-value: static get() -> u32;")));
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("static-value: static set(v: u32);"))
        );
        assert!(
            functions
                .iter()
                .any(|item| item.signature.as_deref() == Some("async-call: async func();"))
        );
        assert!(functions.iter().all(|item| !item.name.contains('[')
            && !item.signature.as_deref().unwrap_or_default().contains('[')));
    }

    #[test]
    fn inline_world_interfaces_keep_their_resolved_type_scope() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let source = "package demo:app; interface shared { record entry { value: u32 } } world app { import clock: interface { use shared.{entry}; read: func() -> entry; } }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path.clone(), source.into())])).unwrap();
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );

        let offset = source.rfind("entry").unwrap();
        assert!(
            analysis
                .visible_types_at(&path, offset)
                .iter()
                .any(|ty| ty.name == "entry"),
            "scopes: {:?}",
            analysis.scopes
        );
        assert!(
            analysis
                .references
                .iter()
                .any(|reference| reference.path == path && reference.range.start == offset),
            "references: {:?}",
            analysis.references
        );
    }

    #[test]
    fn nested_named_type_references_include_handle_and_alias_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.wit");
        let source = "package test:app; interface shared { record entry { value: u32 } resource connection; resource token; } interface api { use shared.{entry, connection, token, entry as local}; use shared.{entry as state}; nested: func(a: list<entry>, b: option<local>, c: borrow<connection>, d: own<token>, e: result<entry, state>, f: tuple<entry, local>); }";
        let analysis =
            analyze_package(dir.path(), &Overlays::from([(path.clone(), source.into())])).unwrap();
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        for text in [
            "list<entry>",
            "option<local>",
            "borrow<connection>",
            "own<token>",
            "result<entry, state>",
            "tuple<entry, local>",
        ] {
            let offset = source.find(text).unwrap();
            let name = if text == "borrow<connection>" || text == "own<token>" {
                offset
                    + if text.starts_with("borrow") {
                        text.find("connection").unwrap()
                    } else {
                        text.find("token").unwrap()
                    }
            } else if text == "option<local>" {
                offset + text.find("local").unwrap()
            } else if text == "result<entry, state>" {
                offset + text.find("state").unwrap()
            } else if text == "tuple<entry, local>" {
                offset + text.find("local").unwrap()
            } else {
                offset + text.find("entry").unwrap()
            };
            assert!(
                analysis
                    .references
                    .iter()
                    .any(|reference| reference.path == path && reference.range.start == name)
            );
        }
        let state_target = source.find("entry as state").unwrap();
        assert!(
            analysis
                .references
                .iter()
                .any(|reference| reference.range.start == state_target)
        );
        let state_alias = source.find("as state").unwrap() + "as ".len();
        assert!(
            analysis
                .references
                .iter()
                .any(|reference| reference.range.start == state_alias)
        );
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
