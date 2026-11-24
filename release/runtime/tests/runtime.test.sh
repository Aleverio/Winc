#!/usr/bin/env bash
# Offline tests for the winctup runtime. No network, no sudo, no release assets required.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT

export WINCT_HOME="$tmp/source-home"
export WINCT_BIN="$tmp/source-bin"
export XDG_DATA_HOME="$tmp/source-xdg"
mkdir -p "$WINCT_HOME" "$WINCT_BIN" "$XDG_DATA_HOME"

# The source guard in runtime.sh keeps main() from running here.
source "$root/release/runtime/runtime.sh"

passed=0
n=0
check() {
  local label=$1
  shift
  if "$@"; then
    printf 'ok - %s\n' "$label"
    passed=$((passed + 1))
  else
    printf 'not ok - %s\n' "$label" >&2
    exit 1
  fi
}

new_home() {
  n=$((n + 1))
  HOME_DIR="$tmp/home$n"
  BIN_DIR="$tmp/bin$n"
  LOCK_DIR="$HOME_DIR/.lock"
  DESKTOP_DIR="$tmp/xdg$n/applications"
  mkdir -p "$HOME_DIR" "$BIN_DIR" "$DESKTOP_DIR"
}

uname() {
  case $1 in
    -s) printf '%s\n' "$FAKE_OS" ;;
    -m) printf '%s\n' "$FAKE_ARCH" ;;
  esac
}

tags_are_classified() {
  valid_tag v1.2.3 stable &&
    ! valid_tag v1.2.3-beta.1 stable &&
    valid_tag v1.2.3-beta.1 beta &&
    ! valid_tag v1.2.3.4 beta &&
    ! valid_tag latest stable
}

asset_info() {
  local info
  info=$(asset_for "$1")
  ASSET_NAME=${info%%$'\n'*}
  ASSET_KIND=${info##*$'\n'}
}

linux_uses_appimage() {
  FAKE_OS=Linux FAKE_ARCH=x86_64
  asset_info 1.2.3
  [[ $ASSET_NAME == Winct-1.2.3-linux-x64.AppImage && $ASSET_KIND == appimage ]]
}

macos_uses_dmg() {
  FAKE_OS=Darwin FAKE_ARCH=arm64
  asset_info 1.2.3
  [[ $ASSET_NAME == Winct-1.2.3-macos-arm64.dmg && $ASSET_KIND == dmg ]]
  FAKE_OS=Darwin FAKE_ARCH=x86_64
  asset_info 1.2.3
  [[ $ASSET_NAME == Winct-1.2.3-macos-x64.dmg && $ASSET_KIND == dmg ]]
}

state_round_trips() {
  new_home
  set_state VERSION 1.2.3
  set_state PREVIOUS 1.2.2
  set_state VERSION 1.2.4
  [[ $(read_state VERSION) == 1.2.4 && $(read_state PREVIOUS) == 1.2.2 ]]
}

switch_is_atomic_and_tracks_previous() {
  new_home
  mkdir -p "$HOME_DIR/releases/1.2.2/app" "$HOME_DIR/releases/1.2.3/app"
  switch_to 1.2.3 ''
  [[ $(readlink "$HOME_DIR/current") == releases/1.2.3/app ]]
  switch_to 1.2.2 1.2.3
  [[ $(readlink "$HOME_DIR/current") == releases/1.2.2/app ]]
  [[ $(read_state PREVIOUS) == 1.2.3 ]]
}

prune_keeps_current_previous_and_limit() {
  new_home
  local version
  for version in 1.0.0 1.1.0 1.2.0 1.3.0; do mkdir -p "$HOME_DIR/releases/$version/app"; done
  KEEP=1
  prune 1.3.0 1.2.0
  [[ -d $HOME_DIR/releases/1.2.0 && -d $HOME_DIR/releases/1.3.0 ]]
  [[ ! -e $HOME_DIR/releases/1.0.0 && ! -e $HOME_DIR/releases/1.1.0 ]]
}

hash_matches_coreutils() {
  new_home
  printf 'winctup' > "$HOME_DIR/sample"
  local expected actual
  expected=$(hash_file "$HOME_DIR/sample")
  actual=$(sha256sum "$HOME_DIR/sample" | awk '{print $1}')
  [[ $expected == "$actual" ]]
}

extracts_appimage_and_launches_it() {
  new_home
  local asset="$HOME_DIR/sample.AppImage" stage="$HOME_DIR/stage"
  cat > "$asset" <<'SCRIPT'
#!/bin/sh
if [ "$1" = "--appimage-extract" ]; then
  mkdir -p squashfs-root/usr/bin
  printf '#!/bin/sh\n[ "$1" = "--version" ] && printf 1.2.3\\n' > squashfs-root/usr/bin/winct
  chmod +x squashfs-root/usr/bin/winct
  cp squashfs-root/usr/bin/winct squashfs-root/usr/bin/winct-pty
  exit 0
fi
printf '1.2.3\n'
SCRIPT
  chmod +x "$asset"
  mkdir -p "$stage"
  ASSET_KIND=appimage
  extract "$asset" "$stage"
  [[ -x $stage/app/winct && -x $stage/app/winct-pty && -x $stage/app/usr/bin/winct ]]

  mkdir -p "$HOME_DIR/releases/1.2.3"
  mv "$stage/app" "$HOME_DIR/releases/1.2.3/app"
  switch_to 1.2.3 ''
  install_launcher
  [[ $("$BIN_DIR/winct" --version) == 1.2.3 ]]
  grep -q 'LD_LIBRARY_PATH' "$BIN_DIR/winct"
  grep -q '^Name=Winct$' "$DESKTOP_DIR/winct.desktop"
}

check 'stable and beta tags are classified' tags_are_classified
check 'Linux resolves to the AppImage asset' linux_uses_appimage
check 'macOS resolves to the DMG asset' macos_uses_dmg
check 'state keys can be read and replaced' state_round_trips
check 'switching releases is atomic and records the previous one' switch_is_atomic_and_tracks_previous
check 'pruning keeps current, previous and the configured limit' prune_keeps_current_previous_and_limit
check 'hashing matches sha256sum' hash_matches_coreutils
check 'an AppImage is extracted and the launcher works' extracts_appimage_and_launches_it

lock_is_only_removed_when_held() {
  new_home
  mkdir -p "$LOCK_DIR"
  LOCK_HELD=0
  cleanup
  [[ -d $LOCK_DIR ]]
  LOCK_HELD=1
  cleanup
  [[ ! -d $LOCK_DIR ]]
  LOCK_HELD=0
}

staging_directory_is_cleaned_up() {
  new_home
  STAGE_DIR="$HOME_DIR/releases/.1.2.3.staging"
  mkdir -p "$STAGE_DIR"
  cleanup
  [[ ! -e $STAGE_DIR ]]
  STAGE_DIR=""
}

launcher_follows_symlinks() {
  new_home
  mkdir -p "$HOME_DIR/releases/1.2.3/app"
  cat > "$HOME_DIR/releases/1.2.3/app/winct" <<'SCRIPT'
#!/bin/sh
printf '1.2.3\n'
SCRIPT
  chmod +x "$HOME_DIR/releases/1.2.3/app/winct"
  switch_to 1.2.3 ''
  install_launcher
  ln -s "$BIN_DIR/winct" "$tmp/linked-winct"
  [[ $("$tmp/linked-winct" --version) == 1.2.3 ]]
}

global_channel_option_is_accepted() {
  CHANNEL=stable
  CHANNEL_SET=0
  YES=0
  main --channel beta help >/dev/null
  [[ $CHANNEL == beta && $CHANNEL_SET == 1 ]]
}

check 'cleanup leaves a lock owned by another process' lock_is_only_removed_when_held
check 'a failed install removes its staging directory' staging_directory_is_cleaned_up
check 'the launcher resolves symlinks' launcher_follows_symlinks
check 'global options are accepted before the command' global_channel_option_is_accepted

printf '%d tests passed\n' "$passed"
