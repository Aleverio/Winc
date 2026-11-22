import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

import { renderPackages } from "./package-manifests.mjs";

const TEMPLATES = resolve("packaging/templates");
const VERSION = "1.2.3";
const NAMES = [
  `Winc-${VERSION}-macos-arm64.dmg`,
  `Winc-${VERSION}-macos-x64.dmg`,
  `Winc-${VERSION}-linux-x64.AppImage`,
  `Winc-${VERSION}-linux-x64.deb`,
  `Winc-${VERSION}-linux-x64.rpm`,
  `winc-${VERSION}-1-x86_64.pkg.tar.zst`,
  `Winc-${VERSION}-windows-x64-setup.exe`,
  `Winc-${VERSION}-windows-x64.msi`,
  `Winc-${VERSION}-windows-x64-portable.zip`,
];

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "winct-manifests-"));
  const assets = join(root, "assets");
  const out = join(root, "generated");
  mkdirSync(assets);
  for (const name of NAMES) writeFileSync(join(assets, name), `contents of ${name}`);
  return { root, assets, out };
}

const hash = (name) => createHash("sha256").update(`contents of ${name}`).digest("hex");

test("renders every package target from the release assets", () => {
  const { root, assets, out } = fixture();
  try {
    renderPackages({ tag: `v${VERSION}`, repo: "Aleverio/Winc", assets, out, templates: TEMPLATES });

    for (const file of [
      "aur/PKGBUILD", "aur/.SRCINFO",
      "homebrew/Casks/winct.rb", "homebrew/Formula/winct.rb",
      "scoop/winct.json",
      "winget/Aleverio.Winct.yaml", "winget/Aleverio.Winct.installer.yaml",
      "winget/Aleverio.Winct.locale-en-US.yaml",
      "checksums.json",
    ]) assert.ok(existsSync(join(out, file)), `missing ${file}`);

    const pkgbuild = readFileSync(join(out, "aur/PKGBUILD"), "utf8");
    assert.match(pkgbuild, /^pkgname=winct$/m);
    assert.match(pkgbuild, new RegExp(`^pkgver=${VERSION}$`, "m"));
    assert.ok(pkgbuild.includes(`sha256sums=('${hash(NAMES[5])}')`));

    const cask = readFileSync(join(out, "homebrew/Casks/winct.rb"), "utf8");
    assert.match(cask, new RegExp(`sha256 "${hash(NAMES[0])}"`));
    assert.match(cask, new RegExp(`sha256 "${hash(NAMES[1])}"`));
    assert.match(cask, /name "Winct Editor"/);

    const formula = readFileSync(join(out, "homebrew/Formula/winct.rb"), "utf8");
    assert.match(formula, new RegExp(hash(NAMES[2])));

    const scoop = JSON.parse(readFileSync(join(out, "scoop/winct.json"), "utf8"));
    assert.equal(scoop.version, VERSION);
    assert.equal(scoop.architecture["64bit"].hash, hash(NAMES[8]));
    assert.equal(scoop.name, "Winct Editor");

    const installer = readFileSync(join(out, "winget/Aleverio.Winct.installer.yaml"), "utf8");
    assert.match(installer, new RegExp(`PackageVersion: ${VERSION}`));
    assert.match(installer, new RegExp(`InstallerSha256: ${hash(NAMES[6])}`));
    assert.match(installer, new RegExp(`InstallerSha256: ${hash(NAMES[7])}`));

    const checksums = JSON.parse(readFileSync(join(out, "checksums.json"), "utf8"));
    assert.equal(checksums.tag, `v${VERSION}`);
    assert.equal(checksums.repo, "Aleverio/Winc");
    assert.equal(checksums.assets.SHA_PORTABLE, hash(NAMES[8]));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("leaves no template placeholders behind", () => {
  const { root, assets, out } = fixture();
  try {
    renderPackages({ tag: `v${VERSION}`, repo: "Aleverio/Winc", assets, out, templates: TEMPLATES });
    for (const target of ["aur", "homebrew", "scoop", "winget"]) {
      const dir = join(out, target);
      for (const name of readdirSyncRecursive(dir)) {
        const text = readFileSync(join(dir, name), "utf8");
        assert.doesNotMatch(text, /\{\{[A-Z0-9_]+\}\}/, `${target}/${name} has an unreplaced placeholder`);
      }
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

function readdirSyncRecursive(dir, prefix = "") {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    return statSync(full).isDirectory()
      ? readdirSyncRecursive(full, `${prefix}${name}/`)
      : [`${prefix}${name}`];
  });
}

test("rejects prerelease tags and missing assets", () => {
  const { root, assets, out } = fixture();
  try {
    assert.throws(() => renderPackages({
      tag: "v1.2.3-beta.1", repo: "Aleverio/Winc", assets, out, templates: TEMPLATES,
    }), /stable release tag/);
    rmSync(join(assets, NAMES[8]));
    assert.throws(() => renderPackages({
      tag: `v${VERSION}`, repo: "Aleverio/Winc", assets, out, templates: TEMPLATES,
    }), /missing release asset/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
