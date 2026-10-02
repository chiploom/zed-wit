# Manual Zed qualification

This is a procedure and a pending result template, not a test report. Record the
exact commit, OS/architecture, Zed version, server version/path, commands, observed
result and artifact path for every executed row. Hosted and GUI results require
fresh evidence; a unit test or configured workflow cannot stand in for them.

## Setup

1. Build the native server with `cargo build -p wit-language-server --release --locked`
   and check the adapter with
   `cargo check -p zed-wit --target wasm32-wasip2 --locked`.
2. In Zed, run **zed: install dev extension** and select the repository root.
   Resolve any conflicting installed WIT extension so the dev extension is active.
3. Configure the server override shown in the project README to the absolute
   native executable path. This is required until matching hosted assets exist.
   Confirm the path is trusted and executable; restart the WIT language server.
4. Open the repository's WIT fixtures and a disposable multi-file WIT package.
   Record the Zed log/server stderr and screenshots in a dated evidence directory.
   Do not place private workspace source or secrets in shared evidence.

## Scenarios

| Scenario | Action | Required observable | Status |
| --- | --- | --- | --- |
| Development install | Install dev extension, open `.wit` | WIT language selected; no extension/query load errors | Pending |
| Highlighting and structure | Open records, resources, variants, async/future/stream/map fixtures | Meaningful captures, brackets, indentation, outline; no query errors | Pending |
| Snippets | Invoke a WIT snippet in an empty file | Valid scaffold and working tab stops | Pending |
| Parser diagnostic | Enter invalid syntax then repair it | Diagnostic has correct range and clears after repair | Pending |
| Resolver diagnostic | Reference an unknown type | Parser-backed error points to the relevant identifier/file | Pending |
| Unsaved sibling overlay | Edit a shared type without saving, then use it from a sibling file | Diagnostics reflect the open buffer and propagate to affected files | Pending |
| Dependency package | Add direct `deps/` package and then break its declaration | Resolution succeeds when valid; correct dependency file gets error | Pending |
| Close/reopen | Break an unsaved buffer, close and reopen | Stale diagnostics clear; disk contents become authoritative | Pending |
| Unicode positions | Put non-ASCII and astral characters before an error | Underline and edits match the negotiated position encoding | Pending |
| Formatting | Format comments, docs and multiline constructs twice | Comments preserved; second format produces no further change | Pending |
| Invalid formatting input | Format incomplete/error-tree input | No destructive edit; useful refusal/error | Pending |
| Unsupported capabilities | Inspect initialize and editor commands | No unsupported semantic navigation/rename/completion advertised | Pending |
| Local override | Configure a trusted explicit native binary | Exact binary launches; no download needed | Pending |
| First hosted install | Remove test install cache after assets are published, restart server | Matching platform/version downloads and checksum passes | Pending |
| Cached install | Restart with the verified installed executable | Server starts using the validated cache behavior | Pending |
| Missing/corrupt asset | Exercise controlled missing/checksum/cache-corruption cases | Invalid cache is discarded and a clean download is attempted; unverified executable never starts | Pending |
| Editor restart | Restart Zed with an open WIT package | Language server reconnects and recomputes diagnostics | Pending |

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
