# Security policy

This project is unreleased. There is no supported published release or promised
response SLA yet. Once a release exists, security fixes target the latest release.

Report a suspected vulnerability through the repository's **Security → Report a
vulnerability** private reporting interface, if enabled. If that interface is
unavailable, open an issue requesting a private reporting channel without posting
vulnerability details, sensitive WIT sources, credentials, or an exploit.
Maintainers must enable private reporting before public release.

## Trust boundaries

WIT documents, dependency files and LSP messages are untrusted input. Analysis
must remain bounded and must not execute workspace code or install dependencies.
A configured local server binary is trusted user configuration and executes with
the editor's privileges. Review the path before using a development override.

The adapter selects a fixed version, platform and HTTPS release origin and checks
the downloaded executable against its SHA-256 sidecar before execution. Both are
served by the same origin: this detects corruption and mismatched assets, but
does not protect against a compromised repository or release account. Maintainers
can verify GitHub artifact attestations separately; the adapter does not verify
those attestations automatically.

Pull-request CI receives read-only repository permissions, does not persist
checkout credentials and has no publishing secrets. Release publishing requires
the protected `release` environment. Environment approvals, branch/tag protection
and private reporting are repository settings that must be configured by a
maintainer; workflow YAML alone cannot enable them. See [publishing](docs/publishing.md).
