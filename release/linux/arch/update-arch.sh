#!/usr/bin/env bash
# Update Winct on Arch Linux / CachyOS without a third-party pacman repository.
#
# The latest stable GitHub Release already contains the Arch package and SHA256SUMS.
# This script downloads that package, checks its checksum, and hands it to pacman.
# Use WINCT_RELEASE_REPO=owner/repo to update from a different fork.
set -euo pipefail

fail() { printf '%s\n' "$*" >&2; exit 1; }

repo=${WINCT_RELEASE_REPO:-Aleverio/Winc}
[[ $repo =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'Invalid WINCT_RELEASE_REPO (expected owner/repo)'
((EUID == 0)) || fail 'Run this script with sudo bash update-arch.sh'
[[ $(uname -m) == x86_64 ]] || fail 'The Winct Arch package currently supports x86_64 only'
. /etc/os-release
[[ ${ID:-} == arch || ${ID:-} == cachyos ]] || fail 'This update script is for Arch Linux and CachyOS'
for command in curl awk sha256sum pacman; do
  command -v "$command" >/dev/null || fail "Missing command $command"
done

# GitHub redirects /releases/latest to /releases/tag/<tag>. Prereleases are skipped.
curl_args=(--proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error
  --connect-timeout 15 --max-time 60 --location)
location=$("${curl_args[@]}" --output /dev/null --write-out '%{url_effective}' \
  "https://github.com/$repo/releases/latest")
location=${location%/}
tag=${location##*/}
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Cannot resolve the latest stable release: $tag"
version=${tag#v}
asset="winct-$version-1-x86_64.pkg.tar.zst"
base="https://github.com/$repo/releases/download/$tag"

workdir=$(mktemp -d)
trap 'rm -rf -- "$workdir"' EXIT
"${curl_args[@]}" --max-time 300 --output "$workdir/$asset" "$base/$asset"
"${curl_args[@]}" --output "$workdir/SHA256SUMS" "$base/SHA256SUMS"

expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$workdir/SHA256SUMS")
[[ $expected =~ ^[0-9a-f]{64}$ ]] || fail "SHA256SUMS has no checksum for $asset"
actual=$(sha256sum "$workdir/$asset" | awk '{print $1}')
[[ $actual == "$expected" ]] || fail 'Downloaded package does not match SHA256SUMS'

printf 'Installing Winct %s from %s\n' "$version" "$tag"
pacman -U "$workdir/$asset"
