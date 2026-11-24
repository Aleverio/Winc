# winctup — the Winct runtime

Winct is distributed through a small runtime instead of AUR/Homebrew/Scoop/winget packages. One
`curl` (or `irm`) bootstrap installs the runtime; from then on `winctup` owns installs, updates,
rollback, self-update and removal. GitHub Releases are the single source of truth: every version,
URL and hash comes from the assets attached to the release, so there is no manifest to bump and no
maintainer account to wait for.

## Install

Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.sh | bash
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.ps1 | iex
```

Both commands download the runtime, verify it against `CHECKSUMS`, place launchers in a user-writable
bin directory and install the newest stable release. `sudo` is never required. Put that bin directory
on `PATH`:

- Linux/macOS: `~/.local/bin` (or `$XDG_BIN_HOME`)
- Windows: `%LOCALAPPDATA%\winct\bin`

The launchers are `winct` (`winct.cmd` on Windows) and a compatibility `winc` (`winc.cmd`) that
points to the same build, so the existing `winc .` command line keeps working. Windows also gets a
`winctup.cmd` shim, so `winctup status` works there as well. The application binary, bundle name,
config directory and URL scheme stay `winc`; only the user-facing name is **Winct**.

To pin the runtime to a reviewable revision, replace `dev` in the URL with a commit SHA and export
the same value as `WINCTT_RUNTIME_REF`.

## Commands

| Command | What it does |
| --- | --- |
| `winctup install [--channel stable\|beta]` | Install the newest release; a no-op if already current. |
| `winctup update [--channel ...]` | Same path as `install` (kept as the familiar name). |
| `winctup status` | Show runtime version, channel, installed/latest versions and paths. |
| `winctup rollback` | Switch back to the previously installed release. |
| `winctup self-update` | Verify and replace the runtime itself. |
| `winctup uninstall --yes` | Remove the runtime, releases and launchers. |
| `winctup doctor` | Check the local tools and print the effective paths. |

On Windows the flags are PowerShell style: `winctup.ps1 -Command status`, `-Channel beta`,
`-Yes` for uninstall. The same commands can be invoked through the installed runtime copy at
`~/.local/share/winct/runtime/runtime.sh` or `%LOCALAPPDATA%\winct\runtime\winctup.ps1` even when the
bin directory is not on `PATH`.

## Layout

```
$WINCTT_HOME                         # default: ~/.local/share/winct | %LOCALAPPDATA%\winct
├── runtime/                        # verified copy of runtime.sh / winctup.ps1 + CHECKSUMS
├── releases/
│   ├── 1.2.3/app/                  # Unix: extracted AppImage/DMG build
│   ├── 1.2.3/winc.exe              # Windows: portable ZIP keeps winc.exe at the root
│   └── 1.2.2/                      # kept for rollback (app/ on Unix, root on Windows)
├── cache/                          # downloads, cleaned after 7 days
├── current                         # symlink (Unix) or version file (Windows)
├── state                           # VERSION, PREVIOUS, CHANNEL, ARTIFACT, INSTALLED_AT
└── .lock/                          # mutex directory, so two updates cannot race
$WINCTT_BIN/winct, winc              # launchers
$XDG_DATA_HOME/applications/winct.desktop  # Linux desktop entry (Unix only)
```

## Update flow

1. Resolve the newest release: `/releases/latest` (stable) or the Releases API (beta, which includes
   prereleases). Tags must be `vX.Y.Z` or `vX.Y.Z-<suffix>`.
2. Pick the asset for the host: Linux x86_64 → `Winct-<version>-linux-x64.AppImage`, macOS arm64/x64
   → `Winct-<version>-macos-{arm64,x64}.dmg`, Windows x86_64 →
   `Winct-<version>-windows-x64-portable.zip`.
3. Download `SHA256SUMS` and the asset over HTTPS with retries and timeouts, then compare the
   SHA-256. A mismatch or a missing entry aborts before anything is installed.
4. Extract into `releases/.<version>.staging/` and smoke-test `winc --version`.
5. Move the staging tree into place, then flip `current` atomically (symlink replacement on Unix,
   temp-file + move on Windows). Record the old version as `PREVIOUS`.
6. Write the launchers, prune releases beyond `WINCTT_KEEP_RELEASES` (the current and previous ones
   are always kept) and delete the download cache.

A failed step leaves the previous release untouched. In-app update notifications remain useful for
discovery, but runtime-managed installs should be updated with `winctup update`.

## Security

- All traffic is HTTPS; curl is restricted to HTTPS protocols and TLS 1.2+.
- The runtime verifies the release asset against the release's own `SHA256SUMS`.
- The bootstrap and `self-update` verify the runtime against `CHECKSUMS` committed in this
  repository, so a truncated or tampered download is rejected.
- `WINCTT_RUNTIME_REF` pins the runtime source; pinning to a commit SHA makes the bootstrap
  reproducible and reviewable.
- The lock directory serialises concurrent installs/updates.
- On macOS the quarantine attribute is removed from the extracted app because it was verified by
  hash; Gatekeeper otherwise blocks a command-line launch.
- `WINCTT_GITHUB_TOKEN` (Windows API calls) only raises the GitHub rate limit; it is never written to
  disk. Releases must be public for unauthenticated installs.

This is integrity, not code signing: the trust anchor is this repository over HTTPS. A compromised
repository or release can produce a valid hash. Signing would require platform credentials and is a
separate decision.

## Environment

| Variable | Default | Purpose |
| --- | --- | --- |
| `WINCTT_RELEASE_REPO` | `Aleverio/Winc` | Repository whose Releases are used. |
| `WINCTT_RUNTIME_REF` | `dev` | Ref used for the bootstrap and runtime `CHECKSUMS`. |
| `WINCTT_CHANNEL` | `stable` | `stable` or `beta`. |
| `WINCTT_HOME` | `~/.local/share/winct` / `%LOCALAPPDATA%\winct` | Runtime root. |
| `WINCTT_BIN` | `~/.local/bin` / `$WINCTT_HOME\bin` | Launcher directory. |
| `WINCTT_KEEP_RELEASES` | `3` | Number of releases retained, including current and previous. |
| `WINCTT_GITHUB_TOKEN` | unset | Optional token for Windows Releases API calls. |

## Relationship to native packages

The existing deb, rpm, DMG, NSIS, MSI, AppImage and Arch assets are still built and published. They
remain available for users who prefer a system package, and the Arch `update-arch.sh` helper still
serves pacman installs. winctup deliberately uses the portable assets: they need no elevation, work
on every supported host, and can be updated without waiting for a distribution repository. No AUR,
Homebrew, Scoop or winget repository is required.

## Development

```bash
bash -n release/runtime/runtime.sh release/runtime/install.sh release/runtime/tests/runtime.test.sh
bash release/runtime/tests/runtime.test.sh
pwsh -NoProfile -File release/runtime/tests/winctup.test.ps1
node release/runtime/checksums.mjs --write   # after editing any runtime payload
node release/runtime/checksums.mjs --check
```

The tests are offline: they source the runtime, stub `uname`, and exercise tag classification,
asset mapping, state transitions, pruning, hashing, extraction and both launchers. The PowerShell
suite dot-sources `winctup.ps1` to test the same invariants on the Windows side. CI runs both suites,
parses all PowerShell files for syntax errors, and checks that `CHECKSUMS` matches the payloads.
