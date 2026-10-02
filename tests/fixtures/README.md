# WIT editing corpus

`current/` is one coherent multi-file WIT package covering current syntax: all
primitive and compound types, resources, constructors/static/instance methods,
aliases, world imports/exports/includes, async functions, streams/futures,
feature annotations, escaped identifiers, and Unicode/nested comments.
`gated/` is likewise one coherent package group covering grammar-supported
proposal syntax: map types, fixed-length lists (`list<u8, 4>`), external IDs and
nested packages. Keeping each directory semantically coherent matters because
the language server analyzes sibling `.wit` files together, matching upstream
WIT package-directory semantics.

Passing editing tests means that the pinned Tree-sitter grammar recognizes a
fixture. Analysis tests separately require the complete `current/` and `gated/`
directories to resolve with upstream `wit-parser`; neither establishes feature
availability in a component runtime.

`grammar-gaps/` deliberately records two different limitations:

- `getters-setters.wit`: current gated `get`/`set` sugar is rejected by this grammar.
- `legacy-named-results.wit`: obsolete named result lists are accepted by this
  grammar although current WIT uses a single result type.

The syntax tests assert both limitations so grammar changes require an explicit
qualification update. Semantic validation belongs to `wit-parser` in analysis.
The reference specification is pinned in `docs/upstream-compatibility.md`.
