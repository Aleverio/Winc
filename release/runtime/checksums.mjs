// Maintain release/runtime/CHECKSUMS in GNU coreutils format so both the shell and PowerShell
// bootstraps can verify the runtime they are about to execute.
//
//   node release/runtime/checksums.mjs --write
//   node release/runtime/checksums.mjs --check
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const directory = dirname(fileURLToPath(import.meta.url));
const files = ["install.sh", "runtime.sh", "install.ps1", "winctup.ps1"];
const checksumsFile = join(directory, "CHECKSUMS");

const expectedLines = files.map((name) => {
  const bytes = readFileSync(join(directory, name));
  return `${createHash("sha256").update(bytes).digest("hex")}  ${name}`;
});

if (process.argv.includes("--write")) {
  writeFileSync(checksumsFile, `${expectedLines.join("\n")}\n`);
  console.log(`wrote ${checksumsFile}`);
} else {
  const actual = readFileSync(checksumsFile, "utf8").trim().split(/\r?\n/).filter(Boolean).sort();
  const expected = [...expectedLines].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    console.error("release/runtime/CHECKSUMS is stale; run node release/runtime/checksums.mjs --write");
    process.exitCode = 1;
  } else {
    console.log("release/runtime/CHECKSUMS is up to date");
  }
}
