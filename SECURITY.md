# Security policy

The latest published release is supported for security fixes. There is no
promised response SLA.

Report a suspected vulnerability through the repository's **Security → Report a
vulnerability** private reporting interface. Do not disclose vulnerability
details, sensitive WIT sources, credentials, or exploit material in a public
issue. If private reporting is unavailable, open an issue requesting a private
reporting channel without including sensitive details. Maintainers should keep
private vulnerability reporting enabled for all supported releases.

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
the protected `release` environment. Branch/tag rulesets, environment protection,
and private reporting are repository settings rather than workflow behavior;
verify them before every release. See [publishing](docs/publishing.md).
