#!/usr/bin/env bash
# One-command Winct bootstrap:
#   curl -fsSL https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.sh | bash
# Use a different channel with arguments after `--`:
#   curl -fsSL .../install.sh | bash -s -- --channel beta
#
# This script is intentionally tiny. It verifies the real runtime against CHECKSUMS before running
# it, so every later install, update and self-update uses authenticated bytes from the repository.
set -euo pipefail

REPO=${WINCTT_RELEASE_REPO:-Aleverio/Winc}
REF=${WINCTT_RUNTIME_REF:-dev}
RAW_BASE="https://raw.githubusercontent.com/$REPO/$REF/release/runtime"
HOME_DIR=${WINCTT_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/winct}
TMP_DIR=""

fail() { printf 'winctup bootstrap: %s\n' "$*" >&2; exit 1; }
info() { printf 'winctup bootstrap: %s\n' "$*"; }
cleanup() { if [[ -n $TMP_DIR && -d $TMP_DIR ]]; then rm -rf -- "$TMP_DIR"; fi; }
trap cleanup EXIT

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  else fail "need sha256sum or shasum to verify the runtime"; fi
}

for command in curl awk mktemp; do
  command -v "$command" >/dev/null 2>&1 || fail "missing command: $command"
done
[[ $REPO =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail "WINCTT_RELEASE_REPO must be owner/repo"
[[ $REF =~ ^[A-Za-z0-9._/-]+$ ]] || fail "WINCTT_RUNTIME_REF has an invalid character"

TMP_DIR=$(mktemp -d)
curl_args=(curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error
  --location --retry 3 --connect-timeout 15 --max-time 60)
"${curl_args[@]}" --output "$TMP_DIR/CHECKSUMS" "$RAW_BASE/CHECKSUMS" ||
  fail "cannot download $RAW_BASE/CHECKSUMS"
"${curl_args[@]}" --output "$TMP_DIR/runtime.sh" "$RAW_BASE/runtime.sh" ||
  fail "cannot download $RAW_BASE/runtime.sh"

expected=$(awk '$2 == "runtime.sh" { print $1 }' "$TMP_DIR/CHECKSUMS")
[[ $expected =~ ^[0-9a-f]{64}$ ]] || fail "CHECKSUMS has no runtime.sh entry"
actual=$(hash_file "$TMP_DIR/runtime.sh")
[[ $actual == "$expected" ]] || fail "runtime.sh does not match CHECKSUMS"

mkdir -p "$HOME_DIR/runtime"
cp "$TMP_DIR/runtime.sh" "$HOME_DIR/runtime/runtime.sh"
chmod +x "$HOME_DIR/runtime/runtime.sh"
info "runtime verified; installing Winct"
exec bash "$HOME_DIR/runtime/runtime.sh" install "$@"
