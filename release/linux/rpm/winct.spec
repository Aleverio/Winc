# RPM package for Fedora, RHEL 8 and newer, and openSUSE Tumbleweed: wraps the binaries already built on the
# release runner, it does not compile. `package.sh` passes `winct_version` and `winct_files`.
Name:           winct
Version:        %{winct_version}
Release:        1
Summary:        A lighter, faster desktop code editor
License:        Apache-2.0
URL:            https://github.com/Aleverio/Winc
ExclusiveArch:  x86_64 aarch64

# Libraries linked at build time (glibc and its symbol versions, libgcc) are found by rpmbuild. The
# window system libraries are loaded at run time and are named here by soname, which Fedora and
# openSUSE both provide under different package names.
Requires:       fontconfig
Requires:       libX11.so.6()(64bit)
Requires:       libX11-xcb.so.1()(64bit)
Requires:       libxcb.so.1()(64bit)
Requires:       libXcursor.so.1()(64bit)
Requires:       libXi.so.6()(64bit)
Requires:       libXrandr.so.2()(64bit)
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libwayland-client.so.0()(64bit)
Recommends:     (dejavu-sans-fonts or dejavu-fonts)
# Optional: "Open in External Terminal" follows the desktop default when this launcher is present,
# and otherwise falls back to a terminal emulator found on PATH.
Recommends:     xdg-terminal-exec

# Ship the release binaries as built: no stripping and no debug package. The remote servers are
# static programs for two architectures, uploaded to other hosts and never run here.
%global debug_package %{nil}
# There is no %changelog to take a build date from; the release notes live on winct.si.
%global source_date_epoch_from_changelog 0
%global __os_install_post %{nil}
%global _build_id_links none
%global __requires_exclude_from ^%{_prefix}/lib/winct/.*$
%global __provides_exclude_from ^%{_prefix}/lib/winct/.*$

%description
A native Rust editor with its own rendering engine, Git integration and terminal.

%install
src=%{winct_files}
install -Dm755 "$src/winct" "$src/winct-pty" -t %{buildroot}%{_bindir}
install -Dm755 "$src"/winct-server-Linux-* -t %{buildroot}%{_prefix}/lib/winct
install -Dm644 "$src/winct.desktop" "$src/winct-url-handler.desktop" \
  -t %{buildroot}%{_datadir}/applications
install -Dm644 "$src/winct.png" %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/winct.png
install -Dm644 "$src/winct.xml" %{buildroot}%{_datadir}/mime/packages/winct.xml
install -Dm644 "$src/project.svg" \
  %{buildroot}%{_datadir}/icons/hicolor/scalable/mimetypes/application-x-winct-project.svg
install -Dm644 "$src/winct-open-file.desktop" "$src/winct-open-folder.desktop" \
  -t %{buildroot}%{_datadir}/kio/servicemenus
install -Dm644 "$src"/winct-open-*.nemo_action -t %{buildroot}%{_datadir}/nemo/actions
install -Dm644 "$src/LICENSE" "$src/NOTICE" -t %{buildroot}%{_datadir}/licenses/winct

%files
%{_bindir}/winct
%{_bindir}/winct-pty
%{_prefix}/lib/winct
%{_datadir}/applications/winct.desktop
%{_datadir}/applications/winct-url-handler.desktop
%{_datadir}/icons/hicolor/256x256/apps/winct.png
%{_datadir}/mime/packages/winct.xml
%{_datadir}/icons/hicolor/scalable/mimetypes/application-x-winct-project.svg
%{_datadir}/kio/servicemenus/winct-open-file.desktop
%{_datadir}/kio/servicemenus/winct-open-folder.desktop
%{_datadir}/nemo/actions/winct-open-*.nemo_action
%{_datadir}/licenses/winct
