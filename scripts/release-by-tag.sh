#!/usr/bin/env bash
set -euo pipefail

# Resolve drafts and published releases by their REST database ID. GitHub's
# /releases/tags/{tag} endpoint is for published releases and returns 404 for
# an unpublished draft even when gh release view can identify that draft.
if [ "$#" -ne 1 ]; then
  echo "usage: $0 <release-tag>" >&2
  exit 2
fi
tag=$1
if [ "\${GH_REPO:-}" != "chiploom/zed-wit" ] || [ -z "\${GH_TOKEN:-}" ] ||
   { [ -n "\${GH_HOST:-}" ] && [ "$GH_HOST" != github.com ]; }; then
  echo "release lookup requires authenticated canonical github.com/chiploom/zed-wit context" >&2
  exit 1
fi
case "$tag" in
  v-extension-*) tag_pattern='^v-extension-[0-9]+\.[0-9]+\.[0-9]+$' ;;
  *) tag_pattern='^v[0-9]+\.[0-9]+\.[0-9]+$' ;;
esac
if ! [[ "$tag" =~ $tag_pattern ]]; then
  echo "invalid release tag for ID lookup" >&2
  exit 2
fi

lookup=$(gh release view "$tag" --repo "$GH_REPO" --json databaseId,tagName)
release_id=$(jq -er --arg tag "$tag" '
  select(.tagName == $tag)
  | .databaseId
  | select(type == "number" and . > 0 and floor == .)
' <<<"$lookup")
release_json=$(gh api --hostname github.com "repos/$GH_REPO/releases/$release_id")
if ! jq -e --argjson id "$release_id" --arg tag "$tag" '
  .id == $id and .tag_name == $tag and (.draft | type) == "boolean"
' <<<"$release_json" >/dev/null; then
  echo "release ID lookup returned inconsistent tag or release identity" >&2
  exit 1
fi
printf '%s\n' "$release_json"
