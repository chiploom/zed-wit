# Manual Zed qualification

This is a procedure and a pending result template, not a test report. Record the
exact commit, OS/architecture, Zed version, server version/path, commands, observed
result and artifact path for every executed row. Hosted and GUI results require
fresh evidence; a unit test or configured workflow cannot stand in for them.

## Automated qualification

Run the repeatable local Zed integration gate from the repository root:

```sh
cargo xtask test-zed
```

The command combines deterministic protocol coverage with real Zed process
qualification. It runs the complete workspace unit/integration suite plus
workspace doctests, including tests that execute the exact temporary mutations in
`tests/manual-zed/`: parser break/repair, unresolved names, unsaved sibling
overlays, close/reopen, dependency failures, Unicode ranges, formatting,
hover/navigation, completion and typo quick fixes. The syntax suite also validates
query captures, outline/bracket structure, snippet expansion and tab-stop layout.

The command then builds the Wasm adapter and exact native server, compiles the
pinned WIT grammar, copies the complete repository `tests/` tree into an isolated
workspace, verifies that every source `.wit` fixture was staged, and asks Zed to
open the workspace plus every staged WIT fixture. It writes an explicit
`.zed/settings.json` binary override to the isolated workspace and does not add
the server directory to `PATH`, so successful startup proves the configured
binary path was honored. Zed is launched with `ZED_STATELESS=1`, `--foreground`,
`--new` and `--user-data-dir`.
Stateless mode bypasses Zed's stable-build single-instance guard and keeps its
databases in memory, so the smoke can coexist with your normal running Zed
session. The run passes only after Zed starts a
new instance of the exact `target/release/wit-language-server` binary and the
isolated logs contain no WIT extension, grammar, query or language-server startup
failure. The same isolated profile/workspace is then launched a second time; a
new exact server PID must start and stop cleanly to qualify editor restart and
reconnection.

A successful run writes
`target/zed-smoke/profile/zed-smoke-report.json` plus foreground logs. The report records the exact fixture count and relative paths passed to Zed,
both server PIDs, restart logs, and a result/evidence entry for every scenario in
the checklist below. Intentionally negative fixtures may still produce their
expected parser diagnostics; the real editor gate checks activation, loading,
explicit binary selection, restart lifecycle and integration failures, while the
exact editor mutations and semantic requests are asserted through the native LSP
protocol suite. Hosted-release scenarios remain explicit `not-run` entries until
matching release assets exist. Override the executable/profile/timeout with `--zed`, `--profile` and
`--timeout-seconds` when needed. Custom profiles must be disposable subdirectories
of the repository `target/` tree because the harness resets them recursively.

The real-editor smoke supports macOS, Linux and Windows. Grammar compilation
uses `WASI_SDK_PATH` first, then Zed's normal extension-build wasi-sdk cache.
If neither exists, install/rebuild a dev extension once in Zed or point
`WASI_SDK_PATH` at a wasi-sdk installation.

Zed does not currently expose a supported CLI API for dispatching arbitrary
editor actions or asserting hover/completion UI contents. Those semantics stay
in deterministic LSP tests, while the real Zed smoke proves extension discovery,
grammar/language activation, adapter loading, binary selection and server startup.

## Automated GUI qualification

Run the real editor interaction gate with:

```sh
cargo xtask test-zed-gui --allow-input-injection true
```

This command first builds and probes the selected synthetic-input backend. If
the host has not granted the required permission or the Linux desktop protocol is
unavailable, it fails before running the expensive editor qualification. After a
successful preflight it runs `cargo xtask test-zed`, then stages a second
disposable stateless Zed profile and drives only that Zed instance through
synthetic keyboard input. The interaction deliberately uses Zed's shipped default
keybindings instead of a test-only keymap: Enter accepts the exact snippet match,
Tab/Shift-Tab traverse snippet placeholders, the platform-default outline and file
finder shortcuts navigate structure/files, and the platform-default save shortcut
persists evidence. It verifies the saved disposable files rather than relying on
screenshots:

- WIT snippet completion is invoked in real Zed;
- forward and reverse snippet tab-stop navigation replaces the expected
  placeholders and reaches the final cursor;
- the real Zed outline UI is searched for record, variant and resource symbols;
- each outline navigation target is marked in the disposable buffer and saved;
- the exact native WIT language server must start and stop cleanly for each GUI
  session; and
- GUI logs are scanned with the same integration-failure rules as the normal
  smoke test.

The command is supported on Zed's desktop operating systems: macOS, Linux and
Windows. It uses Enigo 0.6.1 for keyboard injection through a separate helper
binary so Linux never broadcasts one synthetic key through multiple desktop
protocols. Auto mode selects an X11-only helper for X11 sessions. For Wayland it
tries a libei-only helper first and, if that clean isolated attempt fails, retries
from a fresh Zed profile with a Wayland-virtual-keyboard-only helper. Override
auto-detection with `--linux-input-backend x11|wayland|libei` when qualifying a
specific Linux backend. The command fails rather than reporting a skipped pass
if the selected desktop interface is unavailable.

macOS requires Accessibility permission for the terminal or runner that invokes
the command. Windows requires Zed and the runner to use compatible integrity
levels so UIPI does not block synthetic input.

Because this command sends real keyboard events, it requires the explicit
`--allow-input-injection true` acknowledgement. Save or close unrelated
foreground applications and do not interact with the desktop while it runs.
Both automated Zed commands write
`session.trust_all_worktrees = true` to the disposable profile's global
`config/settings.json` before launch. This prevents the isolated test worktree
from entering Restricted Mode, which would otherwise suppress project settings
and language-server startup. If Zed displays an "Unrecognized Project" /
"Trust and Continue" prompt during qualification, treat the run as invalid and
investigate the harness instead of clicking through it manually. All Zed
settings and edited WIT files used by this gate live under the disposable
`target/zed-gui/` profile/workspace; the test does not install or override user
keybindings or persist trust in the user's normal Zed profile.

The test intentionally does not compare theme-specific rendered pixel colors.
Semantic highlight capture correctness remains asserted deterministically by the
Tree-sitter query tests, while real Zed activation and structural presentation
are qualified through successful fixture loading and outline navigation. This
avoids theme, font rasterization, scaling and GPU differences turning aesthetic
pixels into a flaky correctness gate.

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
| Development install | Install dev extension, open `.wit` | WIT language selected; no extension/query load errors | Automated |
| Highlighting and structure | Open records, resources, variants, async/future/stream/map fixtures | Meaningful captures, brackets, indentation, outline; no query errors | Automated; real outline UI covered by `test-zed-gui` |
| Snippets | Invoke a WIT snippet in an empty file | Valid scaffold and working tab stops | Automated end-to-end by `test-zed-gui` |
| Parser diagnostic | Enter invalid syntax then repair it | Diagnostic has correct range and clears after repair | Automated |
| Resolver diagnostic | Reference an unknown type | Parser-backed error points to the relevant identifier/file | Automated |
| Unsaved sibling overlay | Edit a shared type without saving, then use it from a sibling file | Diagnostics reflect the open buffer and propagate to affected files | Automated |
| Dependency package | Add direct `deps/` package and then break its declaration | Resolution succeeds when valid; correct dependency file gets error | Automated |
| Close/reopen | Break an unsaved buffer, close and reopen | Stale diagnostics clear; disk contents become authoritative | Automated |
| Unicode positions | Put non-ASCII and astral characters before an error | Underline and edits match the negotiated position encoding | Automated |
| Formatting | Format comments, docs and multiline constructs twice | Comments preserved; second format produces no further change | Automated |
| Invalid formatting input | Format incomplete/error-tree input | No destructive edit; useful refusal/error | Automated |
| Semantic hover/navigation | Open a named type and a type use, request hover, definition and references | Hover shows resolved declaration; definition targets its source; references include uses and optionally declaration | Automated |
| Context completion | Request completion after a partial type name in valid and invalid documents, and at unsupported declaration positions | Type context offers WIT primitives and only visible types; unsupported contexts do not leak global declarations | Automated |
| Type typo quick fix | Reference a uniquely similar missing named type, then an ambiguous or non-type unresolved name | Only the unique named-type typo gets a source-ranged quick fix | Automated |
| Unsupported capabilities | Inspect initialize and editor commands | No unsupported rename or workspace symbols advertised | Automated |
| Local override | Configure a trusted explicit native binary | Exact binary launches; no download needed; Zed server info reports the expected `+git.<commit>` build identity and View Logs contains the matching startup INFO message | Automated |
| First hosted install | Run `cargo xtask test-zed-hosted` with a fresh isolated profile | Matching published platform/version downloads, checksum passes, and exact release build identity launches | Automated post-release |
| Cached install | Continue the same `test-zed-hosted` run | Second launch reuses the verified executable/checksum without rewriting the cache | Automated post-release |
| Missing/corrupt cache | Continue the same `test-zed-hosted` run | Corrupt executable and missing checksum are rejected; clean published bytes/sidecar are restored before launch | Automated post-release |
| Editor restart | Restart Zed with an open WIT package | Language server reconnects and recomputes diagnostics | Automated |

Run the hosted-install rows separately on macOS ARM64, macOS Intel, Linux ARM64
GNU, Linux x86_64 GNU and Windows x86_64 MSVC when those hosts are available.
`test-zed-hosted` uses an isolated profile, removes WIT language-server entries
from `PATH`, omits any project-local binary override, and records
`target/zed-hosted/profile/zed-hosted-report.json`. Do not claim
editor/platform hosted-delivery qualification from cross-compilation alone.

## Remaining manual surface

The former snippet and outline GUI spot checks are automated by
`cargo xtask test-zed-gui --allow-input-injection true`. Theme-specific pixel
appearance may still be inspected manually when desired, but it is presentation
evidence rather than a correctness or merge gate because semantic captures are
covered by deterministic query tests.

Release delivery is qualified after publication with
`cargo xtask test-zed-hosted`. The command exercises first download, cached
reuse, corrupted executable recovery, and missing-checksum recovery against the
published release that matches `extension.toml`. Platform-specific execution
evidence is still required; the automation does not turn one host into evidence
for another architecture or operating system.

Do not substitute a manual GUI pass when `test-zed-gui` fails because the host
denies input injection. Treat that as an environment/platform qualification
failure and resolve the platform permission/session backend instead. Do not
repeat semantic correctness or the documented temporary file mutations manually
when `cargo xtask test-zed` already passed them.

## Evidence record

For each row retain: scenario name; exact input/fixture; invocation or editor
actions; expected result; observed pass/fail; nonempty screenshot/log/trace paths;
and relevant versions/commit. Add defects and rerun affected scenarios after
fixes. Link the final records from the release review; keep this reusable checklist
separate from any dated report.
