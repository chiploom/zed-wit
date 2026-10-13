#!/usr/bin/env bash
set -euo pipefail

# Offline deterministic manifest capture and read-only GitHub REST verification.
# capture|verify <lsp|extension> <manifest> <dist> <source> [tag title draft|published]
if [ "$#" -lt 5 ]; then
  echo "usage: $0 capture|verify lsp|extension manifest dist source [tag title draft|published]" >&2
  exit 2
fi
operation=$1
scope=$2
manifest=$3
dist=$4
source=$5
case "$scope" in lsp|extension) ;; *) echo "invalid release scope" >&2; exit 2 ;; esac
case "$operation" in capture|verify) ;; *) echo "invalid integrity operation" >&2; exit 2 ;; esac

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 -- "$1" | cut -d ' ' -f 1
  fi
}

expected_names() {
  if [ "$scope" = extension ]; then return; fi
  printf '%s\n' LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md
  for target in \
    aarch64-apple-darwin \
    x86_64-apple-darwin \
    aarch64-unknown-linux-gnu \
    x86_64-unknown-linux-gnu \
    x86_64-pc-windows-msvc
  do
    binary="wit-language-server-$target"
    if [ "$target" = x86_64-pc-windows-msvc ]; then binary="$binary.exe"; fi
    printf '%s\n' "$binary" "$binary.sha256" "$binary.provenance.json" "$binary.licenses.txt"
  done
}

collect_local() {
  local name path digest size
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    case "$name" in LICENSE-MIT|LICENSE-APACHE|THIRD_PARTY_NOTICES.md) path="$source/$name" ;; *) path="$dist/$name" ;; esac
    if [ ! -f "$path" ] || [ -L "$path" ]; then
      echo "missing or nonregular local release asset: $name" >&2
      return 1
    fi
    size=$(wc -c <"$path" | tr -d '[:space:]')
    if ! [[ "$size" =~ ^[0-9]+$ ]] || [ "$size" -eq 0 ]; then
      echo "empty or malformed local release asset: $name" >&2
      return 1
    fi
    digest=$(sha256_file "$path")
    if ! [[ "$digest" =~ ^[0-9a-f]{64}$ ]]; then
      echo "cannot calculate SHA-256 for local release asset: $name" >&2
      return 1
    fi
    jq -nc --arg name "$name" --arg digest "sha256:$digest" --argjson size "$size" \
      '{name:$name,digest:$digest,size:$size}'
  done < <(expected_names)
}

if [ "$operation" = capture ]; then
  if [ "$#" -ne 5 ] || [ -e "$manifest" ]; then
    echo "capture requires five arguments and must not overwrite a manifest" >&2
    exit 1
  fi
  local_json=$(collect_local | jq -s 'sort_by(.name)')
  if [ "$scope" = lsp ]; then expected_count=23; else expected_count=0; fi
  jq -e --argjson count "$expected_count" 'length == $count' <<<"$local_json" >/dev/null
  ( set -o noclobber; printf '%s\n' "$local_json" >"$manifest" )
  echo "captured $expected_count local release asset digests"
  exit 0
fi

if [ "$#" -ne 8 ]; then
  echo "verification requires tag, title, and draft|published state" >&2
  exit 2
fi
tag=$6
title=$7
phase=$8
case "$phase" in draft|published) ;; *) echo "invalid release phase" >&2; exit 2 ;; esac
if [ "${GH_REPO:-}" != "chiploom/zed-wit" ] || [ -z "${GH_TOKEN:-}" ] ||
   { [ -n "${GH_HOST:-}" ] && [ "$GH_HOST" != github.com ]; }; then
  echo "release asset verification requires authenticated canonical github.com/chiploom/zed-wit context" >&2
  exit 1
fi
case "$scope" in
  lsp) tag_pattern='^v[0-9]+\.[0-9]+\.[0-9]+$' ;;
  extension) tag_pattern='^v-extension-[0-9]+\.[0-9]+\.[0-9]+$' ;;
esac
if ! [[ "$tag" =~ $tag_pattern ]]; then
  echo "release tag does not match the validated scope" >&2
  exit 2
fi

captured=$(cat -- "$manifest")
current=$(collect_local | jq -s 'sort_by(.name)')
if ! jq -ne --argjson a "$captured" --argjson b "$current" '$a == $b'; then
  echo "locally verified release assets changed after digest capture" >&2
  exit 1
fi

release_json=$(gh api --hostname github.com "repos/$GH_REPO/releases/tags/$tag")
release_id=$(jq -er '.id | select(type=="number" and . > 0 and floor == .)' <<<"$release_json")
# --slurp retains all API pages as nested arrays; incomplete pages fail closed.
pages_json=$(gh api --hostname github.com --paginate --slurp "repos/$GH_REPO/releases/$release_id/assets?per_page=100")

if ! jq -ne \
  --argjson release "$release_json" \
  --argjson pages "$pages_json" \
  --argjson expected "$captured" \
  --arg tag "$tag" --arg title "$title" --arg phase "$phase" --arg scope "$scope" '
  def valid_asset:
    (type == "object")
    and (.id | type == "number" and . > 0 and floor == .)
    and (.name | type == "string" and length > 0)
    and (.state == "uploaded")
    and (.size | type == "number" and . > 0 and floor == .)
    and (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$"));
  def identity: {id,name,size,state,digest};
  ($release | type == "object")
  and ($release.tag_name == $tag)
  and ($release.name == $title)
  and ($release.draft == ($phase == "draft"))
  and (if $phase == "published" then $release.immutable == true else true end)
  and ($release.assets | type == "array")
  and ($pages | type == "array" and length > 0 and all(.[];type == "array" and length <= 100))
  and (($pages | add) | type == "array")
  and ($expected | type == "array" and length == (if $scope == "lsp" then 23 else 0 end))
  and ([$expected[].name] | unique | length == ($expected | length))
  and (all($expected[]; (.name | type == "string") and (.size | type == "number" and . > 0) and (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$"))))
  and ($release.assets | length == ($expected | length))
  and (($pages | add) | length == ($expected | length))
  and (all($release.assets[]; valid_asset))
  and (all(($pages | add)[]; valid_asset))
  and ([$release.assets[].name] | unique | length == ($expected | length))
  and ([($pages | add)[].name] | unique | length == ($expected | length))
  and ([$release.assets[].id] | unique | length == ($expected | length))
  and ([($pages | add)[].id] | unique | length == ($expected | length))
  and (([$release.assets[] | identity] | sort_by(.name)) ==
       ([($pages | add)[] | identity] | sort_by(.name)))
  and (([$release.assets[] | {name,size,digest}] | sort_by(.name)) == ($expected | sort_by(.name)))
  ' >/dev/null; then
  echo "GitHub release assets differ from captured SHA-256, size or uploaded state" >&2
  exit 1
fi
echo "verified $phase release assets against captured local SHA-256 digests ($scope)"
