// Render package-manager manifests from a published GitHub Release.
//
// Usage:
//   node release/common/package-manifests.mjs \
//     --tag v1.2.3 --repo Aleverio/Winc --assets <dir with release assets> --out packaging/generated
//
// The workflow downloads the assets with `gh release download` first, so the hashes here are the
// exact bytes users will receive. Templates live in packaging/templates.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const ASSETS = {
  dmgArm64: (v) => `Winc-${v}-macos-arm64.dmg`,
  dmgX64: (v) => `Winc-${v}-macos-x64.dmg`,
  appimage: (v) => `Winc-${v}-linux-x64.AppImage`,
  deb: (v) => `Winc-${v}-linux-x64.deb`,
  rpm: (v) => `Winc-${v}-linux-x64.rpm`,
  arch: (v) => `winc-${v}-1-x86_64.pkg.tar.zst`,
  setup: (v) => `Winc-${v}-windows-x64-setup.exe`,
  msi: (v) => `Winc-${v}-windows-x64.msi`,
  portable: (v) => `Winc-${v}-windows-x64-portable.zip`,
};

const TARGETS = ["aur", "homebrew", "scoop", "winget"];

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function fail(message) {
  throw new Error(message);
}

function renderPackages({ tag, repo, assets, out, templates }) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) fail(`expected a stable release tag, got ${tag}`);
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo)) fail(`invalid repository ${repo}`);
  const version = tag.slice(1);

  const values = { VERSION: version, TAG: tag, REPO: repo };
  for (const [key, nameOf] of Object.entries(ASSETS)) {
    const name = nameOf(version);
    const file = join(assets, name);
    if (!existsSync(file) || !statSync(file).isFile()) fail(`missing release asset: ${name}`);
    const placeholder = key.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase();
    values[`URL_${placeholder}`] = `https://github.com/${repo}/releases/download/${tag}/${name}`;
    values[`SHA_${placeholder}`] = sha256(file);
  }

  const substitute = (text) => text.replace(/\{\{([A-Z0-9_]+)\}\}/g, (_, key) => {
    if (!(key in values)) fail(`unknown placeholder ${key}`);
    return values[key];
  });

  const root = resolve(out);
  const renderDirectory = (sourceDir, destinationDir) => {
    mkdirSync(destinationDir, { recursive: true });
    for (const name of readdirSync(sourceDir).sort()) {
      const source = join(sourceDir, name);
      const destination = join(destinationDir, name);
      if (statSync(source).isDirectory()) {
        renderDirectory(source, destination);
      } else if (statSync(source).isFile()) {
        writeFileSync(destination, substitute(readFileSync(source, "utf8")));
      }
    }
  };
  for (const target of TARGETS) {
    const dir = join(templates, target);
    if (!existsSync(dir) || !statSync(dir).isDirectory()) fail(`missing templates for ${target}`);
    renderDirectory(dir, join(root, target));
  }

  // Keep a machine-readable record so other automation can find the assets without re-parsing.
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "checksums.json"), `${JSON.stringify({
    tag, repo, version,
    assets: Object.fromEntries(Object.entries(values).filter(([key]) => key.startsWith("SHA_") || key.startsWith("URL_"))),
  }, null, 2)}\n`);
  return values;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2);
    const value = (flag) => args.includes(flag) ? args[args.indexOf(flag) + 1] : undefined;
    for (const flag of ["--tag", "--repo", "--assets", "--out"]) if (!value(flag)) fail(`${flag} is required`);
    const templates = value("--templates") ?? resolve("packaging/templates");
    const values = renderPackages({
      tag: value("--tag"), repo: value("--repo"), assets: resolve(value("--assets")),
      out: value("--out"), templates: resolve(templates),
    });
    console.log(`Rendered manifests for ${values.VERSION} from ${values.REPO}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

export { renderPackages };
