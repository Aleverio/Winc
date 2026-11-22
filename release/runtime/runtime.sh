#!/usr/bin/env bash
# winctup - the Winct install/update runtime for Linux and macOS.
#
# A curl bootstrap installs this script; from then on it owns the lifecycle: resolve a release,
# verify its SHA-256, extract into a staging directory, atomically flip `current`, keep the previous
# release for rollback, and prune old ones. No sudo, no package repository, no hand-edited version.
set -euo pipefail

RUNTIME_VERSION="0.1.0"
REPO=${WINCT_RELEASE_REPO:-Aleverio/Winc}
REF=${WINCT_RUNTIME_REF:-dev}
RAW_BASE="https://raw.githubusercontent.com/$REPO/$REF/release/runtime"
HOME_DIR=${WINCT_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/winct}
BIN_DIR=${WINCT_BIN:-${XDG_BIN_HOME:-$HOME/.local/bin}}
LOCK_DIR="$HOME_DIR/.lock"
DESKTOP_DIR=${XDG_DATA_HOME:-$HOME/.local/share}/applications

CHANNEL=${WINCT_CHANNEL:-stable}
CHANNEL_SET=0
KEEP=${WINCT_KEEP_RELEASES:-3}
YES=0
ASSET_KIND=""
TMP_DIR=""

CURL=(curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error
  --location --retry 3 --connect-timeout 15)

fail() { printf 'winctup: %s\n' "$*" >&2; exit 1; }
info() { printf 'winctup: %s\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || fail "missing command: $1"; }

cleanup() {
  if [[ -n $TMP_DIR && -d $TMP_DIR ]]; then rm -rf -- "$TMP_DIR"; fi
  if [[ -d $LOCK_DIR ]]; then rmdir "$LOCK_DIR" 2>/dev/null || true; fi
}
trap cleanup EXIT

shell_quote() {
  local value=$1
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//\$/\\\$}
  value=${value//\`/\\\`}
  printf '"%s"' "$value"
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    fail "need sha256sum or shasum to verify downloads"
  fi
}

curl_dl() {
  "${CURL[@]}" --max-time 600 --output "$2" "$1" || fail "download failed: $1"
  [[ -s $2 ]] || fail "downloaded an empty file: $1"
}

read_state() {
  [[ -f "$HOME_DIR/state" ]] || return 0
  awk -F= -v key="$1" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$HOME_DIR/state"
}

set_state() {
  local key=$1 value=$2 tmp="$HOME_DIR/.state.$$"
  if [[ -f "$HOME_DIR/state" ]]; then
    awk -F= -v key="$key" -v value="$value" '
      $1 == key { print key "=" value; seen = 1; next }
      { print }
      END { if (!seen) print key "=" value }
    ' "$HOME_DIR/state" > "$tmp"
  else
    printf '%s=%s\n' "$key" "$value" > "$tmp"
  fi
  mv -f "$tmp" "$HOME_DIR/state"
}

acquire_lock() {
  mkdir -p "$HOME_DIR"
  local waited=0
  until mkdir "$LOCK_DIR" 2>/dev/null; do
    waited=$((waited + 1))
    ((waited < 60)) || fail "another winctup process holds the lock"
    sleep 1
  done
}

# Stable means X.Y.Z only. Beta accepts a prerelease suffix from the same tag family.
valid_tag() {
  local tag=$1 channel=$2
  if [[ $channel == stable ]]; then
    [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
  else
    [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]
  fi
}

latest_tag() {
  local channel=$1 tag json
  if [[ $channel == stable ]]; then
    tag=$("${CURL[@]}" --max-time 60 --output /dev/null --write-out '%{url_effective}' \
      "https://github.com/$REPO/releases/latest")
    tag=${tag%/}
    tag=${tag##*/}
  else
    json=$("${CURL[@]}" --max-time 60 -H 'Accept: application/vnd.github+json' \
      "https://api.github.com/repos/$REPO/releases?per_page=30")
    tag=$(printf '%s\n' "$json" |
      grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -n 1 |
      sed 's/.*"\([^"]*\)"$/\1/' || true)
  fi
  valid_tag "$tag" "$channel" ||
    fail "cannot resolve the latest $channel release (got '$tag')"
  printf '%s\n' "$tag"
}

# Prints "<asset name>\n<artifact kind>". Callers capture both lines: a function cannot set a
# global from inside a command substitution.
asset_for() {
  local version=$1 os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Linux:x86_64) printf 'Winc-%s-linux-x64.AppImage\nappimage\n' "$version" ;;
    Darwin:arm64) printf 'Winc-%s-macos-arm64.dmg\ndmg\n' "$version" ;;
    Darwin:x86_64) printf 'Winc-%s-macos-x64.dmg\ndmg\n' "$version" ;;
    *) fail "unsupported platform: $os $arch" ;;
  esac
}

extract() {
  local archive=$1 stage=$2
  case $ASSET_KIND in
    appimage)
      need chmod
      chmod +x "$archive"
      (cd "${archive%/*}" && "./${archive##*/}" --appimage-extract >/dev/null)
      [[ -d "${archive%/*}/squashfs-root/usr/bin" ]] || fail "AppImage did not extract a usr/ tree"
      mkdir -p "$stage/app"
      cp -a "${archive%/*}/squashfs-root/usr" "$stage/app/"
      ln -s usr/bin/winc "$stage/app/winc"
      ln -s usr/bin/winc-pty "$stage/app/winc-pty"
      ;;
    dmg)
      need hdiutil
      local mount="${archive%/*}/mount"
      mkdir -p "$mount"
      hdiutil attach -nobrowse -readonly "$archive" -mountpoint "$mount" >/dev/null
      mkdir -p "$stage/app"
      if ! cp -a "$mount/winc.app" "$stage/app/"; then
        hdiutil detach "$mount" >/dev/null 2>&1 || true
        fail "the dmg has no winc.app"
      fi
      hdiutil detach "$mount" >/dev/null 2>&1 || true
      xattr -dr com.apple.quarantine "$stage/app" 2>/dev/null || true
      ln -s winc.app/Contents/MacOS/winc "$stage/app/winc"
      ln -s winc.app/Contents/MacOS/winc-pty "$stage/app/winc-pty"
      ;;
    *) fail "unknown artifact kind: $ASSET_KIND" ;;
  esac
  [[ -x "$stage/app/winc" ]] || fail "the extracted release has no winc executable"
  if [[ $ASSET_KIND == appimage ]]; then
    local lib="$stage/app/usr/lib"
    LD_LIBRARY_PATH="$lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      "$stage/app/winc" --version >/dev/null 2>&1 || fail "the extracted executable failed its smoke check"
  else
    "$stage/app/winc" --version >/dev/null 2>&1 || fail "the extracted executable failed its smoke check"
  fi
}

install_launcher() {
  mkdir -p "$BIN_DIR"
  local root lib LD
  root=$(shell_quote "$HOME_DIR")
  if [[ $ASSET_KIND == appimage ]]; then
    lib=$(shell_quote "$HOME_DIR/current/app/usr/lib")
    # $LD is expanded into the heredoc once; the escaped ${...} stays for the generated shell.
    LD="LD_LIBRARY_PATH=$lib"\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}$'\n'"export LD_LIBRARY_PATH"
  else
    LD=''
  fi
  cat > "$BIN_DIR/winct" <<EOF
#!/bin/sh
set -eu
root=$root
$LD
exec "\$root/current/winc" "\$@"
EOF
  cat > "$BIN_DIR/winc" <<EOF
#!/bin/sh
set -eu
exec "\$(dirname "\$0")/winct" "\$@"
EOF
  chmod +x "$BIN_DIR/winct" "$BIN_DIR/winc"

  if [[ $ASSET_KIND == appimage ]]; then
    mkdir -p "$DESKTOP_DIR"
    cat > "$DESKTOP_DIR/winct.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Winct
Comment=Winct Editor - a lighter, faster desktop code editor
Exec=$(shell_quote "$BIN_DIR/winct") %F
Icon=$HOME_DIR/current/app/usr/share/icons/hicolor/256x256/apps/winc.png
Terminal=false
Categories=Development;IDE;
EOF
  fi
  info "launchers: $BIN_DIR/winct, $BIN_DIR/winc"
}

switch_to() {
  local version=$1 previous=$2 link="$HOME_DIR/.current.new"
  if [[ -e $HOME_DIR/current && ! -L $HOME_DIR/current ]]; then
    # A pre-runtime install may have left a real directory here.
    mv "$HOME_DIR/current" "$HOME_DIR/.current.legacy.$$" || fail "cannot move the old current/ directory"
  fi
  rm -f "$link"
  ln -s "releases/$version/app" "$link"
  mv -f "$link" "$HOME_DIR/current"
  set_state VERSION "$version"
  set_state PREVIOUS "$previous"
  set_state INSTALLED_AT "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}

prune() {
  local current=$1 previous=$2 entry name retained=0 sorted
  local -a versions=()
  for entry in "$HOME_DIR"/releases/*; do
    [[ -d $entry ]] || continue
    name=${entry##*/}
    [[ $name =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || continue
    versions+=("$name")
  done
  if ((${#versions[@]})); then
    # Newest first: current and previous are always retained, older ones fill the remaining KEEP.
    sorted=$(printf '%s\n' "${versions[@]}" | sort -t. -k1,1nr -k2,2nr -k3,3nr)
    while IFS= read -r name; do
      [[ -n $name ]] || continue
      if [[ $name == "$current" || $name == "$previous" ]]; then
        retained=$((retained + 1))
        continue
      fi
      if ((retained >= KEEP)); then
        rm -rf -- "$HOME_DIR/releases/$name"
      else
        retained=$((retained + 1))
      fi
    done <<< "$sorted"
  fi
  find "$HOME_DIR/cache" -type f -mtime +7 -exec rm -f {} + 2>/dev/null || true
}

do_install() {
  need curl
  need awk
  [[ $REPO =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail "WINCT_RELEASE_REPO must be owner/repo"
  [[ $REF =~ ^[A-Za-z0-9._/-]+$ ]] || fail "WINCT_RUNTIME_REF has an invalid character"
  [[ $KEEP =~ ^[0-9]+$ ]] || fail "WINCT_KEEP_RELEASES must be a number"
  mkdir -p "$HOME_DIR"/{releases,cache}
  acquire_lock

  local channel=$CHANNEL
  if ((CHANNEL_SET == 0)); then
    channel=$(read_state CHANNEL)
    [[ -n $channel ]] || channel=stable
  fi
  [[ $channel == stable || $channel == beta ]] || fail "unknown channel: $channel"

  local tag version asset asset_info base expected actual old stage
  tag=$(latest_tag "$channel")
  version=${tag#v}
  old=$(read_state VERSION)
  if [[ $old == "$version" && -e "$HOME_DIR/current" && $channel == "$(read_state CHANNEL)" ]]; then
    info "Winct $version is already installed ($channel channel)"
    return 0
  fi

  asset_info=$(asset_for "$version")
  asset=${asset_info%%$'\n'*}
  ASSET_KIND=${asset_info##*$'\n'}
  base="https://github.com/$REPO/releases/download/$tag"
  TMP_DIR=$(mktemp -d "$HOME_DIR/cache/.tmp.XXXXXX")
  stage="$HOME_DIR/releases/.$version.staging"
  rm -rf "$stage"
  mkdir -p "$stage"

  curl_dl "$base/SHA256SUMS" "$TMP_DIR/SHA256SUMS"
  curl_dl "$base/$asset" "$TMP_DIR/$asset"
  expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$TMP_DIR/SHA256SUMS")
  [[ $expected =~ ^[0-9a-f]{64}$ ]] || fail "SHA256SUMS has no entry for $asset"
  actual=$(hash_file "$TMP_DIR/$asset")
  [[ $actual == "$expected" ]] || fail "download does not match SHA256SUMS"

  info "installing Winct $version from $tag"
  extract "$TMP_DIR/$asset" "$stage"
  rm -rf -- "$HOME_DIR/releases/$version"
  mv "$stage" "$HOME_DIR/releases/$version"
  switch_to "$version" "$old"
  set_state CHANNEL "$channel"
  set_state ARTIFACT "$asset"
  install_launcher
  prune "$version" "$old"
  rm -rf -- "$TMP_DIR"
  TMP_DIR=""
  info "Winct $version is ready"
}

do_status() {
  local installed channel latest
  installed=$(read_state VERSION)
  channel=$(read_state CHANNEL)
  [[ -n $channel ]] || channel=stable
  latest=$(latest_tag "$channel")
  printf 'runtime:   winctup %s\n' "$RUNTIME_VERSION"
  printf 'channel:   %s\n' "$channel"
  printf 'installed: %s\n' "${installed:-none}"
  printf 'latest:    %s\n' "$latest"
  printf 'home:      %s\n' "$HOME_DIR"
  printf 'bin:       %s\n' "$BIN_DIR"
}

do_rollback() {
  acquire_lock
  local current previous
  current=$(read_state VERSION)
  previous=$(read_state PREVIOUS)
  [[ -n $previous && -d "$HOME_DIR/releases/$previous/app" ]] ||
    fail "no previous release is available to roll back to"
  switch_to "$previous" "$current"
  info "rolled back to Winct $previous"
}

do_self_update() {
  need curl
  need awk
  [[ $REF =~ ^[A-Za-z0-9._/-]+$ ]] || fail "WINCT_RUNTIME_REF has an invalid character"
  TMP_DIR=$(mktemp -d)
  curl_dl "$RAW_BASE/CHECKSUMS" "$TMP_DIR/CHECKSUMS"
  curl_dl "$RAW_BASE/runtime.sh" "$TMP_DIR/runtime.sh"
  local expected actual
  expected=$(awk '$2 == "runtime.sh" { print $1 }' "$TMP_DIR/CHECKSUMS")
  [[ $expected =~ ^[0-9a-f]{64}$ ]] || fail "CHECKSUMS has no runtime.sh entry"
  actual=$(hash_file "$TMP_DIR/runtime.sh")
  [[ $actual == "$expected" ]] || fail "runtime.sh does not match CHECKSUMS"
  mkdir -p "$HOME_DIR/runtime"
  cp "$TMP_DIR/runtime.sh" "$HOME_DIR/runtime/runtime.sh"
  cp "$TMP_DIR/CHECKSUMS" "$HOME_DIR/runtime/CHECKSUMS"
  chmod +x "$HOME_DIR/runtime/runtime.sh"
  rm -rf -- "$TMP_DIR"
  TMP_DIR=""
  info "winctup runtime updated and verified"
}

do_uninstall() {
  ((YES == 1)) || fail "pass --yes to confirm uninstall"
  [[ ${HOME_DIR##*/} =~ ^winct(up)?$ ]] ||
    fail "refusing to remove $HOME_DIR: WINCT_HOME must end in winct or winctup"
  rm -rf -- "$HOME_DIR"
  rm -f -- "$BIN_DIR/winct" "$BIN_DIR/winc" "$DESKTOP_DIR/winct.desktop"
  info "winctup and its releases were removed; application settings were kept"
}

do_doctor() {
  local command
  for command in curl awk grep mktemp; do need "$command"; done
  case "$(uname -s)" in
    Linux) need sha256sum ;;
    Darwin) need shasum; need hdiutil ;;
  esac
  printf 'runtime: winctup %s\n' "$RUNTIME_VERSION"
  printf 'home:    %s\n' "$HOME_DIR"
  printf 'bin:     %s\n' "$BIN_DIR"
  printf 'state:   %s\n' "$(read_state VERSION || printf none)"
  info "environment looks healthy"
}

usage() {
  cat <<'EOF'
winctup - install and update Winct from GitHub Releases

Usage: winctup <command> [options]

Commands:
  install [--channel stable|beta]   install Winct (no-op when already current)
  update  [--channel stable|beta]   check for a release and install it
  status                            show installed, latest, channel and paths
  rollback                          return to the previous release
  self-update                       verify and replace this runtime script
  uninstall --yes                   remove the runtime and its launchers
  doctor                            check the local environment

Options:
  --channel <name>   release channel (default: stable)
  --yes              confirm destructive commands
  -h, --help         show this help
EOF
}

main() {
  local command=${1:-help}
  [[ $command == -* ]] && { usage; return 0; }
  shift || true
  while (($#)); do
    case $1 in
      --channel)
        (($# >= 2)) || fail "--channel needs a value"
        CHANNEL=$2
        CHANNEL_SET=1
        shift 2
        ;;
      --yes) YES=1; shift ;;
      -h|--help) usage; return 0 ;;
      *) fail "unknown option: $1" ;;
    esac
  done
  case $command in
    install|update) do_install ;;
    status) do_status ;;
    rollback) do_rollback ;;
    self-update) do_self_update ;;
    uninstall) do_uninstall ;;
    doctor) do_doctor ;;
    help) usage ;;
    *) fail "unknown command: $command (try 'winctup help')" ;;
  esac
}

# Sourcing this file exposes the helpers to the test suite without running a command.
if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  main "$@"
fi
