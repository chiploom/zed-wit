# WIT editing corpus

Each leaf directory under this corpus is an independent WIT package root. This
matches upstream directory semantics and the language server's sibling-file
analysis model; unrelated package fixtures must not share a directory.

`current/` covers current syntax across independent packages: all primitive and
compound types, resources, constructors/static/instance methods, aliases, world
imports/exports/includes, async functions, streams/futures, feature annotations,
escaped identifiers, and Unicode/nested comments.

`gated/` covers grammar-supported proposal syntax in independent packages: map
types, fixed-length lists (`list<u8, 4>`), external IDs and nested packages.

Passing editing tests means that the pinned Tree-sitter grammar recognizes a
fixture. Analysis tests separately require every `current/` and `gated/`
package directory to resolve with upstream `wit-parser`; neither establishes
feature availability in a component runtime.

`grammar-gaps/` deliberately records two different limitations as independent
packages:

- `getters-setters/getters-setters.wit`: current gated `get`/`set` sugar is
  rejected by the pinned Tree-sitter grammar but accepted by `wit-parser`.
- `legacy-named-results/legacy-named-results.wit`: obsolete named result lists
  are accepted by the grammar although current `wit-parser` rejects them.

The syntax and analysis tests assert both limitations so grammar/parser changes
require an explicit qualification update. The reference specification is pinned
in `docs/upstream-compatibility.md`.
