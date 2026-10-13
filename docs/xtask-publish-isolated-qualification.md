# Isolated release qualification

This document separates the safety properties demonstrated by offline,
production-code integration tests from the properties that require a real
GitHub Actions environment. It is not a release procedure and authorizes no
publication.

## Production CLI process qualification

Run `bash scripts/test-publish-cli-process.sh` on a trusted development
machine or CI runner with Rust 1.99.0 and an offline Cargo dependency cache.

The harness clones the current source into a temporary Git repository,
compiles its actual `xtask` executable inside the clone, and invokes
`cargo xtask publish`. It uses a local bare origin, synthetic GitHub CLI
responses, and a Git shim that **rejects every production CLI push**.
Unknown GitHub API operations fail closed. It does not reuse a production
checkout for writes, inject a test-only publish flag, or invoke protected CD.

It covers both LSP and extension streams: real planning and preparation,
confirmation and option parsing, unprepared submission rejection, missing PR
identifier rejection, clean/dirty worktree enforcement, and proof that
preparation does not commit or push the candidate. The existing connected
Rust lifecycle tests separately exercise shared production submit and resume
transport helpers, post-review provenance, merged-tree integrity,
incomplete submission and recovery, failed/pending CD outcomes, and an
ambiguous dispatch response.

**Not yet executed as an entire production process:** human-reviewed
`--submit` after its full dependency/security preflight, post-merge
`--resume` through its complete live policy/PR-file/remote checks, and
exact protected-CD dispatch. A mock GH CLI can exercise those paths only
after the full request/response surface is emulated. Do not claim a complete
real-GitHub E2E test on the strength of the helper tests.

## Protected CD qualification boundary

`.github/workflows/release.yml` builds five native LSP targets, packages
binaries and redistribution notices, uploads per-target artifacts, downloads
them with `merge-multiple: true`, verifies the complete distribution set,
then attests the binary and sidecar set using the workflow's `attest_lsp`
job. Published asset SHA-256, byte sizes, count, uploaded states and names
are checked before draft promotion and after immutable publication. The
isolated shell fixture executes the production asset integrity verifier with
23 synthetic LSP assets and zero extension assets.

A mock or static workflow test can qualify expected configuration and
transitions but **cannot prove** GitHub-hosted artifact attestation,
OIDC token issuance, native ABI correctness, or immutable publication.
Those require an explicitly authorized, nonproduction release
qualification on all five native runners. Do not treat skipped PR artifact
jobs as successful release artifact attestation.

## Read-only release-policy token check

`RELEASE_POLICY_READ_TOKEN` is an environment-scoped secret. Its existence
does not prove runtime authorization. Do not retrieve, print, copy, or
exchange this secret for local testing.

An authorized administrator can later approve a **separate read-only
workflow** on protected `main` with `workflow_dispatch`, a
`release`-environment job and only `contents: read` GitHub Actions
permissions. Unlike production CD, it must have **no release, tag,
artifact, deployment, push, or dispatch writes**, and no shell debugging
(`set -x`). The minimal verification step is:

```yaml
environment: release
permissions:
  contents: read
steps:
  - name: Verify immutable-release policy with environment credential
    env:
      GH_TOKEN: ${{ secrets.RELEASE_POLICY_READ_TOKEN }}
      GH_REPO: ${{ github.repository }}
    run: |
      set -euo pipefail
      test -n "${GH_TOKEN:-}" || { echo "::error::Missing policy credential"; exit 1; }
      test "$GH_REPO" = "chiploom/zed-wit"
      response=$(gh api --hostname github.com         -H "X-GitHub-Api-Version: 2026-03-10"         "repos/$GH_REPO/immutable-releases" 2>/dev/null) || {
          echo "::error::Read-only policy request denied"
          exit 1
        }
      jq -e '.enabled == true and (.enforced_by_owner | type) == "boolean"'         <<<"$response" >/dev/null || {
          echo "::error::Policy response malformed or immutable enforcement off"
          exit 1
        }
      echo "Verified immutable-release policy read access"
```

This is a **proposed procedure only**, not an added workflow or an executed
credential test. The admin should inspect the workflow diff and ensure
that it cannot trigger `release.yml` or acquire write permissions before
authorizing execution. Return only the exit status and policy state, never
the credential or complete headers. A successful request proves that
the supplied token can read the endpoint at that moment; it does not
establish minimal token privilege by itself. An authorized operator must
inspect the token's fine-grained permission configuration independently.

## Concurrency and recovery limitations

- A local `resume` lock cannot serialize different worktrees or machines.
  Protected CD concurrency and strict release provenance are the final
  safety boundary; they do not provide atomic exactly-once dispatch.
- The GitHub workflow dispatch response may be lost after acceptance. Do
  not blindly retry ambiguous outcomes or infer success from a nearby run.
- Draft assets can be mutated between the final REST SHA-256 check and
  publication. The immutable final read detects discrepancies but cannot
  roll them back after publication. Treat a discrepancy as an incident.
- Real workflow cancellation, forced replacement, GitHub Actions
  attestations, and cross-checkout duplicate dispatch remain unqualified
  until a deliberately isolated protected-workflow test is authorized.

Neither the production release workflow nor GitHub repository settings
should be changed just to obtain green offline tests.
