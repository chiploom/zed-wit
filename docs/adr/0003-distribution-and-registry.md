# ADR 0003: Version native releases and treat registry succession separately

Status: accepted, 2026-10-01.

Distribute five raw native executables at a fixed versioned GitHub release URL,
with individual SHA-256 sidecars. Verify bytes before execution; support a local
binary override for development. End users do not need Cargo. SHA-256 delivered
from the same origin detects corruption, not compromise of that origin. Release
attestations supply additional independently inspectable provenance.

Build with the pinned Rust toolchain on native runners. Publish only a validated
existing `vX.Y.Z` tag at the default branch dispatch commit, using the protected
`release` environment. Do not replace an existing release's assets.

The registry `wit` entry already belongs to `valentinegb/zed-wit`. Development
installation is independent of registry eligibility. A successor requires owner
permission or written contact attempts unanswered for six weeks, followed by Zed
maintainer agreement; no competing registry entry or transfer is assumed.
