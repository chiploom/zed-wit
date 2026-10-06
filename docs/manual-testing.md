# Manual Zed qualification

This is a procedure and a pending result template, not a test report. Record the
exact commit, OS/architecture, Zed version, server version/path, commands, observed
result and artifact path for every executed row. Hosted and GUI results require
fresh evidence; a unit test or configured workflow cannot stand in for them.

## Setup

1. Build the native server with `cargo build -p wit-language-server --release --locked`
   and run `./target/release/wit-language-server --version` (append `.exe` on
   Windows). Record the reported `+git.<commit>` build identity. Check the
   adapter with `cargo build --target wasm32-wasip2 --locked`. This intentionally
   mirrors Zed's package selection when it compiles a Rust dev extension from
   the workspace root.
2. In Zed, run **zed: install dev extension** and select the repository root.
   Resolve any conflicting installed WIT extension so the dev extension is active.
3. Copy `.zed/settings.example.json` to the ignored
   `.zed/settings.json`. The project-local path is relative to the repository
   worktree; append `.exe` on Windows. This override is required until matching
   hosted assets exist. Confirm the path is trusted and executable, then restart
   the WIT language server.
4. Open each leaf package under `tests/fixtures/current/` and
   `tests/fixtures/gated/`. Each leaf directory is an independent package root
   and should produce no parser or resolver diagnostics. The leaf packages under
   `tests/fixtures/grammar-gaps/` deliberately exercise known grammar/parser
   disagreements and are not expected to be uniformly clean.
5. Use the committed reusable packages under `tests/manual-zed/` for semantic,
   overlay, dependency, Unicode and formatting qualification. Follow
   [their README](../tests/manual-zed/README.md) for the exact temporary edits,
   then restore the baselines with `git restore tests/manual-zed`.
6. Record the Zed language-server log, server info and screenshots in a dated
   evidence directory. Do not place private workspace source or secrets in shared
   evidence.

## Reusable manual fixtures

The manual fixture packages are intentionally valid at rest. Tests that require
invalid syntax, unresolved names, unsaved overlays or corrupt declarations are
performed as temporary editor mutations so the repository never carries broken
WIT as its baseline.

| Fixture | Primary scenarios |
| --- | --- |
| `tests/manual-zed/semantic/` | completion, hover, definition, references, typo quick fixes, negative completion context |
| `tests/manual-zed/escaped/` | explicit `%` identifier spelling across completion, hover and navigation |
| `tests/manual-zed/escaped-alias/` | imported escaped aliases and definition/source spelling |
| `tests/manual-zed/overlay/` | unsaved sibling overlays and close/reopen behavior |
| `tests/manual-zed/dependency/` | direct `deps/` package resolution and dependency diagnostics |
| `tests/manual-zed/unicode/` | UTF-16/Unicode diagnostic range alignment |
| `tests/manual-zed/formatting/` | formatting, comment preservation, idempotence and invalid-input refusal |

See `tests/manual-zed/README.md` for the exact expected observations and
temporary edits. Restore the directory after each qualification pass.

## Scenarios

| Scenario                   | Action                                                                                                                | Required observable                                                                                                                                                     | Status  |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Development install        | Install dev extension, open `.wit`                                                                                    | WIT language selected; no extension/query load errors                                                                                                                   | Pending |
| Highlighting and structure | Open records, resources, variants, async/future/stream/map fixtures                                                   | Meaningful captures, brackets, indentation, outline; no query errors                                                                                                    | Pending |
| Snippets                   | Invoke a WIT snippet in an empty file                                                                                 | Valid scaffold and working tab stops                                                                                                                                    | Pending |
| Parser diagnostic          | Enter invalid syntax then repair it                                                                                   | Diagnostic has correct range and clears after repair                                                                                                                    | Pending |
| Resolver diagnostic        | Reference an unknown type                                                                                             | Parser-backed error points to the relevant identifier/file                                                                                                              | Pending |
| Unsaved sibling overlay    | Edit a shared type without saving, then use it from a sibling file                                                    | Diagnostics reflect the open buffer and propagate to affected files                                                                                                     | Pending |
| Dependency package         | Add direct `deps/` package and then break its declaration                                                             | Resolution succeeds when valid; correct dependency file gets error                                                                                                      | Pending |
| Close/reopen               | Break an unsaved buffer, close and reopen                                                                             | Stale diagnostics clear; disk contents become authoritative                                                                                                             | Pending |
| Unicode positions          | Put non-ASCII and astral characters before an error                                                                   | Underline and edits match the negotiated position encoding                                                                                                              | Pending |
| Formatting                 | Format comments, docs and multiline constructs twice                                                                  | Comments preserved; second format produces no further change                                                                                                            | Pending |
| Invalid formatting input   | Format incomplete/error-tree input                                                                                    | No destructive edit; useful refusal/error                                                                                                                               | Pending |
| Semantic hover/navigation  | Open a named type and a type use, request hover, definition and references                                            | Hover shows resolved declaration; definition targets its source; references include uses and optionally declaration                                                     | Pending |
| Context completion         | Request completion after a partial type name in valid and invalid documents, and at unsupported declaration positions | Type context offers WIT primitives and only visible types; unsupported contexts do not leak global declarations                                                         | Pending |
| Type typo quick fix        | Reference a uniquely similar missing named type, then an ambiguous or non-type unresolved name                        | Only the unique named-type typo gets a source-ranged quick fix                                                                                                          | Pending |
| Unsupported capabilities   | Inspect initialize and editor commands                                                                                | No unsupported rename or workspace symbols advertised                                                                                                                   | Pending |
| Local override             | Configure a trusted explicit native binary                                                                            | Exact binary launches; no download needed; Zed server info reports the expected `+git.<commit>` build identity and View Logs contains the matching startup INFO message | Pending |
| First hosted install       | Remove test install cache after assets are published, restart server                                                  | Matching platform/version downloads and checksum passes                                                                                                                 | Pending |
| Cached install             | Restart with the verified installed executable                                                                        | Server starts using the validated cache behavior                                                                                                                        | Pending |
| Missing/corrupt asset      | Exercise controlled missing/checksum/cache-corruption cases                                                           | Invalid cache is discarded and a clean download is attempted; unverified executable never starts                                                                        | Pending |
| Editor restart             | Restart Zed with an open WIT package                                                                                  | Language server reconnects and recomputes diagnostics                                                                                                                   | Pending |

Run the install/download rows separately on macOS ARM64, macOS Intel, Linux ARM64
GNU, Linux x86_64 GNU and Windows x86_64 MSVC. Keep pending rows explicit when the
platform or release is unavailable. Do not claim editor/platform support from
cross-compilation alone.

## Evidence record

For each row retain: scenario name; exact input/fixture; invocation or editor
actions; expected result; observed pass/fail; nonempty screenshot/log/trace paths;
and relevant versions/commit. Add defects and rerun affected scenarios after
fixes. Link the final records from the release review; keep this reusable checklist
separate from any dated report.
