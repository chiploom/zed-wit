#!/usr/bin/env bash
set -euo pipefail
root=$(git rev-parse --show-toplevel)
real_git=$(command -v git)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/bin"
export REAL_GIT="$real_git" FIXTURE_ROOT="$tmp"
export GIT_TERMINAL_PROMPT=0 GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export CARGO_NET_OFFLINE=true GH_TOKEN=offline-synthetic GH_REPO=chiploom/zed-wit
export GITHUB_ACTIONS=false MOCK_FAIL_PR_CREATE=0
cat >"$tmp/bin/git" <<'GIT'
#!/usr/bin/env bash
set -euo pipefail
if [ "$1" = remote ] && [ "$2" = get-url ]; then
  echo 'https://github.com/chiploom/zed-wit.git'
  exit 0
fi
if [ "$1" = ls-remote ] && [ "$2" = --get-url ]; then
  echo "$3"
  exit 0
fi
if [ "$1" = ls-remote ] && [ "$2" = --heads ] &&
   [ "$3" = 'https://github.com/chiploom/zed-wit.git' ]; then
  shift 3
  exec "$REAL_GIT" ls-remote --heads "$FIXTURE_ROOT/remote.git" "$@"
fi
if [ "$1" = push ]; then
  # Redirect *only* the exact-commit creation lease used by production.
  if [ "$2" != --porcelain ] || [ "$4" != 'https://github.com/chiploom/zed-wit.git' ]; then
    echo 'fixture refuses unmatched push destination' >&2
    exit 90
  fi
  case "$3" in
    --force-with-lease=refs/heads/release-prep/*:) ;;
    *) echo 'fixture refuses noncreation push lease' >&2; exit 91 ;;
  esac
  case "$5" in
    HEAD:refs/heads/release-prep/*) ;;
    *) echo 'fixture refuses unrelated Git push ref' >&2; exit 92 ;;
  esac
  exec "$REAL_GIT" push "$2" "$3" "$FIXTURE_ROOT/remote.git" "$5"
fi
exec "$REAL_GIT" "$@"
GIT
cat >"$tmp/bin/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  "config list") echo 'api_host=api.github.com'; exit 0 ;;
  "auth status") echo 'offline fixture'; exit 0 ;;
  "repo view") echo '{"nameWithOwner":"chiploom/zed-wit","defaultBranchRef":{"name":"main"}}'; exit 0 ;;
  "pr list") echo '[]'; exit 0 ;;
  "pr create")
    if [ "$MOCK_FAIL_PR_CREATE" = 1 ]; then
      echo 'synthetic lost response after local branch creation' >&2
      exit 98
    fi
    echo 'https://github.com/chiploom/zed-wit/pull/314'
    exit 0
    ;;
esac
endpoint=
for endpoint in "$@"; do :; done
if [ "$1" = api ] && [ "$2" = --hostname ] && [ "$3" = github.com ]; then
  case "$endpoint" in
    repos/chiploom/zed-wit/releases?*) exit 0 ;;
    repos/chiploom/zed-wit/rules/branches/main?*)
      echo '{"type":"pull_request","parameters":{"required_approving_review_count":0}}'
      echo '{"type":"required_linear_history"}'
      jq -nc '{type:"required_status_checks",parameters:{
        strict_required_status_checks_policy:true,
        required_status_checks:(
          ["quality","Tests / aarch64-apple-darwin",
          "Tests / x86_64-unknown-linux-gnu","Tests / x86_64-pc-windows-msvc",
          "Check / aarch64-unknown-linux-gnu","Check / x86_64-apple-darwin"]
          | map({context:.,integration_id:15368})
        )}}'
      exit 0
      ;;
    repos/chiploom/zed-wit/rulesets/700?*)
      jq -nc '{id:700,target:"tag",enforcement:"active",bypass_actors:[],
        conditions:{ref_name:{include:["refs/tags/v*"],exclude:[]}},
        rules:[{type:"update"},{type:"deletion"},{type:"non_fast_forward"}]}'
      exit 0
      ;;
    repos/chiploom/zed-wit/rulesets?*)
      echo '{"id":700}'
      exit 0
      ;;
    repos/chiploom/zed-wit/pulls/314)
      head=$("$REAL_GIT" -C "$FIXTURE_ROOT/repo" rev-parse "refs/heads/$FIXTURE_BRANCH")
      jq -nc --arg branch "$FIXTURE_BRANCH" --arg sha "$head" \
        '{base:{ref:"main"},head:{ref:$branch,sha:$sha,
        repo:{full_name:"chiploom/zed-wit"}}}'
      exit 0
      ;;
  esac
fi
echo "fixture denied GitHub operation: $*" >&2
exit 77
GH
chmod +x "$tmp/bin/git" "$tmp/bin/gh"
"$real_git" clone --local --no-hardlinks --quiet "$root" "$tmp/repo"
"$real_git" init --bare --quiet "$tmp/remote.git"
"$real_git" -C "$tmp/remote.git" config receive.shallowUpdate true
"$real_git" -C "$tmp/repo" switch -C main >/dev/null
"$real_git" -C "$tmp/repo" config user.name 'Offline CLI Fixture'
"$real_git" -C "$tmp/repo" config user.email 'offline@example.invalid'
"$real_git" -C "$tmp/repo" config commit.gpgsign false
"$real_git" -C "$tmp/repo" config gc.auto 0
"$real_git" -C "$tmp/repo" config maintenance.auto false
"$real_git" -C "$tmp/repo" remote set-url origin "$tmp/remote.git"
"$real_git" -C "$tmp/repo" push -q origin main
base=$("$real_git" -C "$tmp/repo" rev-parse main)
export PATH="$tmp/bin:$PATH" CARGO_TARGET_DIR="$tmp/build"
cd "$tmp/repo"
cargo build --offline --locked -p xtask
reject() {
  local label=$1 expected=$2
  shift 2
  if "$@" >"$tmp/out" 2>"$tmp/err"; then
    echo "unexpected successful CLI operation: $label" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected" "$tmp/err"; then
    echo "unexpected CLI rejection: $label" >&2
    cat "$tmp/err" >&2
    exit 1
  fi
}
for scope in lsp extension; do
  "$real_git" reset --hard -q "$base"
  "$real_git" clean -fdq
  "$real_git" switch -C main "$base" >/dev/null
  reject "confirmation $scope" 'require --confirm' cargo xtask publish --scope "$scope" --prepare
  reject "argument $scope" 'unknown or duplicate publish option' cargo xtask publish --scope "$scope" --nonexistent
  reject "unprepared submit $scope" 'expected release-preparation branch' cargo xtask publish --scope "$scope" --submit --confirm
  reject "missing resume PR $scope" '--pr is required exactly' cargo xtask publish --scope "$scope" --resume --confirm
  cargo xtask publish --scope "$scope" --dry-run >"$tmp/plan.json"
  jq -e --arg stream "$scope" '.phase=="planned" and .scope==$stream and .remote_writes==false' "$tmp/plan.json" >/dev/null
  cargo xtask publish --scope "$scope" --prepare --confirm >"$tmp/prepared"
  grep -Fq 'No branch was pushed' "$tmp/prepared"
  [ "$("$real_git" rev-parse HEAD)" = "$base" ]
  [ -n "$("$real_git" status --porcelain)" ]
  branch=$("$real_git" symbolic-ref -q --short HEAD)
  case "$branch" in release-prep/"$scope"-*) ;; *) exit 1 ;; esac
  [ -z "$("$real_git" ls-remote --heads "$tmp/remote.git" "$branch")" ]
  reject "unreviewed submit $scope" 'publish requires a clean working tree' cargo xtask publish --scope "$scope" --submit --confirm

  candidate=$(jq -r '.candidate_version' "$tmp/plan.json")
  notes="docs/releases/$scope/v$candidate.md"
  printf '# Reviewed %s release %s\n\n' "$scope" "$candidate" >"$notes"
  printf '%s\n' 'The proposed release was reviewed in the isolated process test.' \
    'The preparation contains only the scope-specific version, lockfile, changelog and notes.' \
    'No real service, GitHub API request, production tag or release was touched.' >>"$notes"
  sed 's/TODO: Replace with reviewed release details/Reviewed isolated process-level qualification/' \
    CHANGELOG.md >"$tmp/changelog"
  cat "$tmp/changelog" >CHANGELOG.md
  "$real_git" add -A
  "$real_git" commit -qm 'human-reviewed synthetic release candidate'
  export FIXTURE_BRANCH="$branch"
  reviewed=$("$real_git" rev-parse HEAD)
  [ -z "$("$real_git" status --porcelain)" ]

  export MOCK_FAIL_PR_CREATE=1
  reject "interrupted PR creation $scope" 'GitHub CLI request failed' \
    cargo xtask publish --scope "$scope" --submit --confirm
  [ "$("$real_git" ls-remote --heads "$tmp/remote.git" "$branch" | cut -f1)" = "$reviewed" ]
  export MOCK_FAIL_PR_CREATE=0
  cargo xtask publish --scope "$scope" --submit --confirm >"$tmp/submitted"
  grep -Fq 'Release preparation PR submitted' "$tmp/submitted"
  [ "$("$real_git" ls-remote --heads "$tmp/remote.git" "$branch" | cut -f1)" = "$reviewed" ]
  [ -z "$("$real_git" ls-remote --tags "$tmp/remote.git")" ]
  echo "passed production CLI plan, prepare, interrupted submission, exact retry: $scope"
done
echo 'passed real CLI subprocess fixture without external writes'
