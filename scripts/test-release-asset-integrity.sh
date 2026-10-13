#!/usr/bin/env bash
set -euo pipefail

# This test executes the exact verifier used by protected CD with a mocked
# gh executable. No network access, credentials or release mutations occur.
root=$(cd "$(dirname "$0")/.." && pwd)
verifier="$root/scripts/release-asset-integrity.sh"
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/dist" "$tmp/source"
export GH_REPO=chiploom/zed-wit GH_TOKEN=synthetic-offline-test-token
export MOCK_RELEASE="$tmp/release.json" MOCK_PAGES="$tmp/pages.json"
export MOCK_API_FAILURE=0
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] || exit 12
if [ "$MOCK_API_FAILURE" = 1 ]; then exit 33; fi
case "$*" in
  *"/releases/tags/"*) cat "$MOCK_RELEASE" ;;
  *"/releases/42/assets?per_page=100"*) cat "$MOCK_PAGES" ;;
  *) exit 22 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH"

for target in \
  aarch64-apple-darwin \
  x86_64-apple-darwin \
  aarch64-unknown-linux-gnu \
  x86_64-unknown-linux-gnu \
  x86_64-pc-windows-msvc
do
  binary="wit-language-server-$target"
  if [ "$target" = x86_64-pc-windows-msvc ]; then binary="$binary.exe"; fi
  printf 'binary %s\n' "$binary" >"$tmp/dist/$binary"
  printf 'checksum %s\n' "$binary" >"$tmp/dist/$binary.sha256"
  printf 'provenance %s\n' "$binary" >"$tmp/dist/$binary.provenance.json"
  printf 'licenses %s\n' "$binary" >"$tmp/dist/$binary.licenses.txt"
done
for asset in LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md; do
  printf '%s\n' "$asset offline fixture" >"$tmp/source/$asset"
done

bash "$verifier" capture lsp "$tmp/lsp-manifest.json" "$tmp/dist" "$tmp/source"
jq -e 'length == 23' "$tmp/lsp-manifest.json" >/dev/null
if bash "$verifier" capture lsp "$tmp/lsp-manifest.json" "$tmp/dist" "$tmp/source" >/dev/null 2>&1; then
  echo "captured manifest was overwritten" >&2; exit 1
fi

make_fixture() {
  jq -n --slurpfile wanted "$tmp/lsp-manifest.json" \
    '{id:42,tag_name:"v0.1.3",name:"WIT Language Server v0.1.3",draft:true,immutable:false,
      assets:[range(0;($wanted[0]|length)) as $i |
        $wanted[0][$i] + {id:($i+1),state:"uploaded"}]}' >"$MOCK_RELEASE"
  jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
}
make_fixture

verify() {
  bash "$verifier" verify lsp "$tmp/lsp-manifest.json" "$tmp/dist" "$tmp/source" \
    v0.1.3 "WIT Language Server v0.1.3" "$1" >/dev/null
}
pass() {
  if ! verify "$1"; then echo "valid $1 release rejected" >&2; exit 1; fi
}
fail() {
  if verify "$1" 2>/dev/null; then echo "unsafe $1 release accepted" >&2; exit 1; fi
}
mutate_asset() {
  local name=$1 expression=$2
  jq --arg name "$name" "(.assets[] | select(.name == \$name)) |= ($expression)" "$MOCK_RELEASE" >"$tmp/next.json"
  mv "$tmp/next.json" "$MOCK_RELEASE"
  jq -e --arg name "$name" '
    (.id == 42) and (.assets | type == "array")
    and ([.assets[] | select(.name == $name)] | length == 1)
  ' "$MOCK_RELEASE" >/dev/null
  jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
}

pass draft
if GH_HOST=enterprise.example.invalid verify draft 2>/dev/null; then
  echo "enterprise host was accepted for GitHub.com publishing" >&2; exit 1
fi
if GH_REPO=other/repository verify draft 2>/dev/null; then
  echo "noncanonical repository was accepted" >&2; exit 1
fi
for item in \
  wit-language-server-aarch64-apple-darwin \
  wit-language-server-aarch64-apple-darwin.sha256 \
  wit-language-server-aarch64-apple-darwin.provenance.json \
  LICENSE-MIT THIRD_PARTY_NOTICES.md
do
  make_fixture
  mutate_asset "$item" '.digest = ("sha256:" + ("f" * 64))'
  fail draft
done

make_fixture
mutate_asset LICENSE-APACHE '.size += 1'
fail draft
make_fixture
mutate_asset LICENSE-MIT '.state = "starter"'
fail draft
make_fixture
mutate_asset LICENSE-MIT '.digest = null'
fail draft
make_fixture
mutate_asset LICENSE-MIT '.digest = "sha256:wrong"'
fail draft
make_fixture
jq '.assets |= .[:-1]' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
fail draft
make_fixture
jq '.assets += [.assets[0]]' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
fail draft
make_fixture
jq '.assets += [{id:100,name:"unexpected",size:1,state:"uploaded",digest:("sha256:"+("a"*64))}]' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
fail draft
make_fixture
echo '[]' >"$MOCK_PAGES"
fail draft
make_fixture
jq '[.assets[:-1]]' "$MOCK_RELEASE" >"$MOCK_PAGES"
fail draft
make_fixture
jq '[.assets, .assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
fail draft
make_fixture
printf 'not json\n' >"$MOCK_PAGES"
fail draft
make_fixture
export MOCK_API_FAILURE=1
fail draft
export MOCK_API_FAILURE=0

# The initial name-only check would pass; an equal-name replacement
# between that check and the final promotion must fail.
make_fixture
pass draft
mutate_asset LICENSE-APACHE '.digest = ("sha256:" + ("c" * 64))'
fail draft
# Safe draft recovery: subsequent successful upload restores identical bytes.
make_fixture
pass draft

# Reject local mutation after capture, even if the API still advertises
# the previously captured bytes.
printf 'unauthorized change\n' >>"$tmp/dist/wit-language-server-x86_64-unknown-linux-gnu"
fail draft
printf 'binary %s\n' wit-language-server-x86_64-unknown-linux-gnu >"$tmp/dist/wit-language-server-x86_64-unknown-linux-gnu"
pass draft

# Immutable published release must carry the same bytes.
jq '.draft = false | .immutable = true' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
pass published
jq '.immutable = false' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
fail published

bash "$verifier" capture extension "$tmp/extension-manifest.json" "$tmp/dist" "$tmp/source"
jq -e 'length == 0' "$tmp/extension-manifest.json" >/dev/null
jq -n '{id:42,tag_name:"v-extension-0.1.3",name:"WIT for Zed Extension v0.1.3",
  draft:true,immutable:false,assets:[]}' >"$MOCK_RELEASE"
printf '[[]]\n' >"$MOCK_PAGES"
verify_extension() {
  bash "$verifier" verify extension "$tmp/extension-manifest.json" "$tmp/dist" "$tmp/source" \
    v-extension-0.1.3 "WIT for Zed Extension v0.1.3" draft >/dev/null
}
verify_extension
jq '.assets += [{id:1,name:"unexpected",state:"uploaded",size:1,digest:("sha256:"+("a"*64))}]' "$MOCK_RELEASE" >"$tmp/next.json"
mv "$tmp/next.json" "$MOCK_RELEASE"
jq '[.assets]' "$MOCK_RELEASE" >"$MOCK_PAGES"
if verify_extension 2>/dev/null; then echo "extension attached assets accepted" >&2; exit 1; fi

echo "passed offline release asset integrity scenarios: LSP 23 assets, corruption, pagination, recovery, immutable state, extension zero-assets"
