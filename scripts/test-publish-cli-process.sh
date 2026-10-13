#!/usr/bin/env bash
set -euo pipefail
root=$(git rev-parse --show-toplevel)
real_git=$(command -v git)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/bin"
export REAL_GIT="$real_git"
export GIT_TERMINAL_PROMPT=0 GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export CARGO_NET_OFFLINE=true GH_TOKEN=offline-synthetic GH_REPO=chiploom/zed-wit
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
if [ "$1" = push ]; then
  echo 'fixture prohibits remote push' >&2
  exit 99
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
esac
endpoint=
for endpoint in "$@"; do :; done
if [ "$1" = api ] && [ "$2" = --hostname ] && [ "$3" = github.com ]; then
  case "$endpoint" in
    repos/chiploom/zed-wit/releases?*) exit 0 ;;
  esac
fi
echo "fixture denied GitHub operation: $*" >&2
exit 77
GH
chmod +x "$tmp/bin/git" "$tmp/bin/gh"
"$real_git" clone --local --no-hardlinks --quiet "$root" "$tmp/repo"
"$real_git" init --bare --quiet "$tmp/remote.git"
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
  reject "unreviewed submit $scope" 'publish requires a clean worktree' cargo xtask publish --scope "$scope" --submit --confirm
  echo "passed isolated production plan/prepare/submit-preflight: $scope"
done
echo 'passed real CLI subprocess fixture without external writes'
