# Manual Zed qualification fixtures

These packages are reusable baselines for the scenarios in
[`docs/manual-testing.md`](../../docs/manual-testing.md). They are intentionally
valid at rest. Create invalid states only as temporary editor mutations, then
restore the committed baselines:

```sh
git restore tests/manual-zed
```

Record the exact repository commit, Zed version, native server build identity,
OS/architecture, observed result and evidence path for every scenario.

## Semantic editor

Open `semantic/main.wit`.

- Hover on either `item` use in `echo` must resolve to `record item`.
- Go to definition from an `item` use must jump to the record declaration.
- Find references with the declaration included should show the declaration and
  both function-signature uses.
- Completion in a type position must include `item` and WIT primitive types.
- Temporarily change one `item` use to `itme`. The unresolved-type
  diagnostic should offer exactly the safe quick fix `Replace with \`item\``.
- For a negative completion check, temporarily replace the function with
  `call: func(first: u32, par)` and request completion at `par`. Type
  completions must not be offered in the parameter-name position.

## Escaped identifiers

Open `escaped/main.wit`.

- Completion at the `%type` use must preserve `%type`, not normalize it to
  `type`.
- Hover on `%func` must preserve `%func`, `%value` and `%type`.
- Definition and references from the `%type` use must target the escaped
  declaration and preserve source spelling.

Open `escaped-alias/main.wit`.

- Completion and hover for `%alias` must preserve the escaped alias.
- Definition from `%type` inside the `use` clause must target the original
  `%type` declaration in `shared`.

## Unsaved sibling overlay and close/reopen

Open both files under `overlay/`.

1. Confirm the package is diagnostic-free.
2. In `types.wit`, rename `item` to `thing` without saving.
3. `main.wit` should report unresolved `item` using the unsaved buffer.
4. Undo the rename without saving; the diagnostic should clear.
5. Rename it again without saving, close `types.wit`, choose **Don't Save**,
   and confirm the disk-backed valid declaration becomes authoritative and stale
   diagnostics clear.

## Dependency package

Open `dependency/main.wit` and `dependency/deps/types.wit`.

- The baseline package must resolve without diagnostics.
- Temporarily change `type item = u32;` to `type item = ;`.
- The resulting diagnostic must be anchored in `deps/types.wit`, not
  `main.wit`.

## Unicode positions

Open `unicode/main.wit`.

- Temporarily change `type broken = u32;` to `type broken = missing;`.
- The diagnostic underline must cover exactly `missing`; the preceding crab
  emoji must not shift the UTF-16 position or any edit range.

## Formatting

Open `formatting/main.wit`.

- The file is deliberately compact but valid. Format it once and verify normal
  spacing/indentation.
- Format it a second time. There must be no further change.

Open `formatting/comments.wit`.

- Format it and verify the line comment, doc comment and annotation survive.
- Run formatting a second time to verify idempotence.

For invalid-input refusal, temporarily remove the final `}` from either
formatting fixture and invoke Format Document. The language server must refuse
the operation rather than return a destructive replacement edit.

## Existing corpus

Highlighting, snippets and grammar-gap behavior continue to use the broader
automated corpus under `tests/fixtures/`. In particular,
`tests/fixtures/grammar-gaps/getters-setters/getters-setters.wit` exercises
accessor recovery and
`tests/fixtures/grammar-gaps/legacy-named-results/legacy-named-results.wit`
must retain the expected upstream parser diagnostic.
