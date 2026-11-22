# Winct Editor packages

Winct is the fork's display name. Internally the crate, binary, configuration directory, URL scheme
and update feeds stay as `winc` so upstream merges remain small. Package identifiers are `winct`,
and package metadata uses **Winct Editor**.

## What is automated

`.github/workflows/package-manifests.yml` runs when a stable GitHub Release is published (or when
dispatched with a release tag). It:

1. downloads the exact assets attached to that release,
2. renders `packaging/templates` into `packaging/generated` with each asset's URL and SHA-256,
3. opens a pull request in this repository carrying the generated manifests, and
4. optionally opens a pull request in each package repository when a token is configured.

The release is the only input. There is no version list to bump and no hash to copy by hand.

Supported targets:

| Target | Generated files | Installs on |
| --- | --- | --- |
| AUR | `aur/PKGBUILD`, `aur/.SRCINFO` | Arch Linux, CachyOS, Manjaro |
| Homebrew tap | `homebrew/Casks/winct.rb` | macOS (Apple silicon and Intel) |
| Homebrew formula | `homebrew/Formula/winct.rb` | Linux via Homebrew |
| Scoop bucket | `scoop/winct.json` | Windows |
| winget fork | `winget/Aleverio.Winct*.yaml` | Windows |

## Rendering locally

The generator needs only the release assets. Point it at a directory containing them:

```bash
gh release download v1.2.3 --dir /tmp/winct-assets --repo Aleverio/Winc
node release/common/package-manifests.mjs \
  --tag v1.2.3 \
  --repo Aleverio/Winc \
  --assets /tmp/winct-assets \
  --out packaging/generated
node --test release/common/package-manifests.test.mjs
```

Every `{{PLACEHOLDER}}` in a template must resolve, and every referenced asset must exist; otherwise
the generator fails instead of writing a broken manifest.

## Publishing to package repositories

Create one repository per target and add tokens as repository secrets. The workflow defaults are:

| Target | Repository variable | Secret | Default repository | Destination |
| --- | --- | --- | --- | --- |
| AUR mirror | `AUR_REPO` | `AUR_TOKEN` | `Aleverio/winct-aur` | repository root |
| Homebrew tap | `HOMEBREW_TAP` | `HOMEBREW_TAP_TOKEN` | `Aleverio/homebrew-winct` | repository root |
| Scoop bucket | `SCOOP_BUCKET` | `SCOOP_BUCKET_TOKEN` | `Aleverio/scoop-winct` | `bucket/` |
| winget fork | `WINGET_FORK` | `WINGET_TOKEN` | `Aleverio/winget-pkgs` | `manifests/a/Aleverio/Winct/<version>/` |

Each token needs `contents: write` and `pull_requests: write` on its repository. A missing token
skips that target; the workflow still succeeds. `release/common/sync-packages.sh` creates a
`winct-<version>` branch in each configured repository, writes the files through the GitHub API and
opens a pull request. Re-running the same release recreates the branch, so it is idempotent.

AUR itself accepts packages by pushing to `aur.archlinux.org`, not to GitHub. Keep the GitHub mirror
as the review location and push the generated `PKGBUILD` and `.SRCINFO` to the AUR from there.
Homebrew taps and Scoop buckets work directly from the GitHub repositories. winget manifests in a
fork still need a pull request to `microsoft/winget-pkgs` and must pass winget's validation.

## Why these targets first

These four are controlled by the fork and can be updated from a release without asking a distro
maintainer. Debian/Ubuntu, Fedora, openSUSE and Nixpkgs require a maintainer account and review;
their manifests can reuse the same generator once those repositories exist.

## Branding boundary

Visible names are `Winct` / `Winct Editor`: the application's About page and window title, desktop
entries, macOS display name and DMG volume, and the Windows installer/shortcut. The command,
`winc.app` bundle, registry keys, environment variables and config directory remain `winc`. This
keeps update URLs and upstream merges working.
