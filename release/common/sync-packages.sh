#!/usr/bin/env bash
# Push generated package manifests to the package repositories and open a PR in each.
#
# Usage:
#   WINC_PACKAGE_TARGETS='AUR|owner/repo|path|TOKEN_VAR ...' \
#     release/common/sync-packages.sh <generated-dir> <version>
#
# Each target is four pipe-separated fields: template name, GitHub repository,
# destination subdirectory inside that repository ({{VERSION}} is expanded), and
# the name of the environment variable holding a token with `contents: write` and
# `pull_requests: write`. Targets without a token are skipped, so the workflow can
# run with no secrets configured.
set -euo pipefail

generated=${1:?generated directory is required}
version=${2:?version is required}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'invalid version: %s\n' "$version" >&2; exit 1; }
for command in gh base64 find; do
  command -v "$command" >/dev/null || { printf 'missing command: %s\n' "$command" >&2; exit 1; }
done

branch="winct-$version"

sync_one() {
  local name=$1 repo=$2 subdir=$3 token_var=$4
  local token=${!token_var:-}
  if [[ -z $token ]]; then
    printf 'Skip %s: %s is not configured\n' "$name" "$token_var"
    return 0
  fi
  local source="$generated/$name"
  if [[ ! -d $source ]]; then
    printf 'Skip %s: no generated manifests in %s\n' "$name" "$source"
    return 0
  fi

  export GH_TOKEN=$token
  local base sha path file relative content
  base=$(gh api "repos/$repo" --jq '.default_branch')
  sha=$(gh api "repos/$repo/git/ref/heads/$base" --jq '.object.sha')

  # Recreate the automation branch so a retried run is idempotent.
  gh api -X DELETE "repos/$repo/git/refs/heads/$branch" >/dev/null 2>&1 || true
  gh api -X POST "repos/$repo/git/refs" -f "ref=refs/heads/$branch" -f "sha=$sha" >/dev/null

  while IFS= read -r -d '' file; do
    relative=${file#"$source"/}
    path=${subdir//\{\{VERSION\}\}/$version}/$relative
    path=${path#./}
    path=${path#/}
    # Feed the JSON on stdin: an installer-sized base64 string is far too large for a CLI argument.
    content=$(base64 -w0 "$file")
    printf '{"message":"chore: update Winct manifests to %s","content":"%s","branch":"%s"}' \
      "$version" "$content" "$branch" |
      gh api -X PUT "repos/$repo/contents/$path" --input - >/dev/null
    printf 'Uploaded %s to %s\n' "$path" "$repo"
  done < <(find "$source" -type f -print0)

  local existing
  existing=$(gh pr list --repo "$repo" --head "$branch" --state open --json number --jq 'length')
  if [[ $existing -gt 0 ]]; then
    printf 'Open pull request already exists in %s for %s\n' "$repo" "$branch"
    return 0
  fi
  gh pr create --repo "$repo" --base "$base" --head "$branch" \
    --title "Winct $version" \
    --body "Generated from https://github.com/${GITHUB_REPOSITORY:-Aleverio/Winc}/releases/tag/v${version} by the package-manifests workflow." >/dev/null
  printf 'Opened pull request in %s for %s\n' "$repo" "$branch"
}

while IFS= read -r target; do
  [[ -n $target ]] || continue
  IFS='|' read -r name repo subdir token_var <<< "$target"
  if [[ -z ${name:-} || -z ${repo:-} || -z ${token_var:-} ]]; then
    printf 'Invalid WINC_PACKAGE_TARGETS entry: %s\n' "$target" >&2
    exit 1
  fi
  sync_one "$name" "$repo" "${subdir:-.}" "$token_var"
done <<< "${WINC_PACKAGE_TARGETS:-}"
