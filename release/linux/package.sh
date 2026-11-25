#!/usr/bin/env bash
# Native Rust packages: an AppImage with bundled libraries, and a system-managed deb.
set -euo pipefail
target="${1:?a Rust target triple is required}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
source release/linux/target.sh
source release/linux/baseline.env
version="$(node release/common/version.mjs)"
deb_version="${version//-/~}"
release="$root/target/$target/release"
bundle="$release/bundle"
tools="$root/target/release-tools"
stem="Winct-$version-$platform"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
appdir="$workdir/winct.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps"
install -m755 "$release/winct" "$release/winct-pty" "$appdir/usr/bin/"
install -m644 "$bundle/icons/winct.png" "$appdir/usr/share/icons/hicolor/256x256/apps/winct.png"
install -m644 release/linux/winct.desktop "$appdir/usr/share/applications/winct.desktop"
desktop-file-validate "$appdir/usr/share/applications/winct.desktop"

# Keep the deb independent of AppImage's patched RPATHs and bundled libraries.
deb="$workdir/deb"
mkdir -p "$deb/DEBIAN" "$deb/usr/share/doc/winct" "$bundle/deb" "$bundle/appimage"
cp -a "$appdir/usr" "$deb/"
# The remote server is uploaded to Linux hosts, never run from here: it lives beside the libraries,
# not on the PATH. Both architectures ship in every package.
servers=("$root/target/remote-server/winct-server-Linux-x86_64" "$root/target/remote-server/winct-server-Linux-aarch64")
install -Dm755 "${servers[@]}" -t "$deb/usr/lib/winct"
# File manager context menus and the winct:// link handler belong to the installed package only: the
# AppImage installs nothing.
install -m644 release/linux/winct-url-handler.desktop "$deb/usr/share/applications/"
# The .winct-project type; the shared-mime-info trigger updates the database.
install -Dm644 release/linux/winct.xml "$deb/usr/share/mime/packages/winct.xml"
install -Dm644 crates/winct-app/assets/file-icons/project.svg \
  "$deb/usr/share/icons/hicolor/scalable/mimetypes/application-x-winct-project.svg"
desktop-file-validate "$deb/usr/share/applications/winct-url-handler.desktop"
install -Dm644 release/linux/file-manager/winct-open-file.desktop release/linux/file-manager/winct-open-folder.desktop \
  -t "$deb/usr/share/kio/servicemenus"
install -Dm644 release/linux/file-manager/*.nemo_action -t "$deb/usr/share/nemo/actions"
install -m644 LICENSE NOTICE "$deb/usr/share/doc/winct/"
cat > "$deb/DEBIAN/control" <<EOF
Package: winct
Version: $deb_version
Section: editors
Priority: optional
Architecture: $deb_arch
Maintainer: DEEIX-AI <41963680+Noxiven@users.noreply.github.com>
Homepage: https://github.com/Aleverio/Winc
Depends: libc6 (>= $GLIBC_FLOOR), libgcc-s1, fontconfig, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxrandr2, libwayland-client0
Recommends: xdg-terminal-exec, fonts-dejavu-core
Description: A lighter, faster desktop code editor
 A native Rust editor with its own rendering engine, Git integration and terminal.
EOF
dpkg-deb --root-owner-group --build "$deb" "$bundle/deb/$stem.deb"

APPIMAGE_EXTRACT_AND_RUN=1 "$tools/linuxdeploy.AppImage" --appimage-extract-and-run \
  --appdir "$appdir" --executable "$appdir/usr/bin/winct" --executable "$appdir/usr/bin/winct-pty" \
  --desktop-file "$appdir/usr/share/applications/winct.desktop" \
  --icon-file "$appdir/usr/share/icons/hicolor/256x256/apps/winct.png" \
  --exclude-library 'libwayland-*' 2>&1 | tee "${LINUXDEPLOY_DEBUG_LOG:-$workdir/linuxdeploy.log}"
# Added after linuxdeploy: it would otherwise treat the static servers as libraries to patch.
install -Dm755 "${servers[@]}" -t "$appdir/usr/lib/winct"
rm -f "$appdir/AppRun"
install -m755 release/linux/AppRun "$appdir/AppRun"
# The AppDir holds servers for two architectures, so appimagetool cannot infer the architecture of
# the AppImage from the files in it and needs it named.
ARCH="$machine" APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool.AppImage" --appimage-extract-and-run \
  --runtime-file "$tools/runtime" "$appdir" "$bundle/appimage/$stem.AppImage"
