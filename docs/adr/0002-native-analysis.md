# ADR 0002: Keep semantic analysis and formatting native

Status: accepted, 2026-10-01.

Implement a small native stdio server, a transport-independent analysis crate,
and a thin Zed Wasm adapter. `wit-parser` 0.260.0 is the semantic authority;
Topiary 0.7.3 formats the concrete syntax tree. Overlay unsaved documents over
bounded sibling-file and `deps/` package discovery.

The audited historical WIT servers offer neither the maintained parser contract
nor the required release assets. A printer for resolved packages cannot preserve
source files and comments. Regex semantics cannot establish symbol identity.

Advertise only implemented, tested capabilities. The supported semantic surface
is diagnostics, document formatting, completion, hover, definition, references and
safe unresolved-type typo quick fixes. Symbol identity comes from `wit-parser`
resolution; syntax nodes supply source token ranges. Rename and workspace symbols
are not advertised.
