"use strict";

// Runs after `npm install kbuildx`. Downloads the prebuilt `kbuildx` binary
// for this platform from the matching GitHub release (tag `kbuildx-v<version>`
// in tsirysndr/vmlinux-builder), verifies its SHA-256, and unpacks it into
// ./binaries/ where the bin shim expects it.
//
// Escape hatches (env vars):
//   KBUILDX_SKIP_DOWNLOAD=1    skip everything (e.g. developing this package)
//   KBUILDX_BINARY=/path       copy a local binary instead of downloading
//   KBUILDX_DOWNLOAD_BASE=url  override the release download base URL

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { execFileSync } = require("node:child_process");
const { detect, binaryPath } = require("./platform.js");

const VERSION = require("../package.json").version;
const REPO = "tsirysndr/vmlinux-builder";

async function main() {
  if (process.env.KBUILDX_SKIP_DOWNLOAD) {
    console.log("[kbuildx] KBUILDX_SKIP_DOWNLOAD set — skipping binary download.");
    return;
  }

  // detect() throws on an unsupported platform/arch — surface that as a failed
  // install with a clear message (the real gate; `npm i -f` bypasses the
  // package.json os/cpu fields).
  const { asset, tag } = detect(VERSION);

  const dest = binaryPath();
  fs.mkdirSync(path.dirname(dest), { recursive: true });

  if (fs.existsSync(dest) && !process.env.KBUILDX_BINARY) {
    console.log(`[kbuildx] binary already present at ${dest}`);
    return;
  }

  if (process.env.KBUILDX_BINARY) {
    fs.copyFileSync(process.env.KBUILDX_BINARY, dest);
    fs.chmodSync(dest, 0o755);
    console.log(`[kbuildx] installed from KBUILDX_BINARY=${process.env.KBUILDX_BINARY}`);
    return;
  }

  const base =
    process.env.KBUILDX_DOWNLOAD_BASE || `https://github.com/${REPO}/releases/download/${tag}`;
  const url = `${base}/${asset}`;

  console.log(`[kbuildx] downloading ${url}`);
  const archive = await download(url, tag);
  await verifyChecksum(archive, `${url}.sha256`, asset);

  const destDir = path.dirname(dest);
  const tgz = path.join(destDir, asset);
  fs.writeFileSync(tgz, archive);
  try {
    // tar + gzip are present on every supported platform (macOS, Linux).
    execFileSync("tar", ["-xzf", tgz, "-C", destDir], { stdio: "inherit" });
  } finally {
    fs.rmSync(tgz, { force: true });
  }
  if (!fs.existsSync(dest)) {
    throw new Error(`archive ${asset} did not contain a 'kbuildx' binary`);
  }
  fs.chmodSync(dest, 0o755);
  console.log(`[kbuildx] installed kbuildx ${VERSION} -> ${dest}`);
}

/** Fetch a URL to a Buffer, following redirects, with a clear error on failure. */
async function download(url, tag) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) {
    throw new Error(
      `failed to download ${url}: HTTP ${res.status} ${res.statusText}.\n` +
        `Is there a published release ${tag} with this asset?\n` +
        `  https://github.com/${REPO}/releases/tag/${tag}`
    );
  }
  return Buffer.from(await res.arrayBuffer());
}

/** Verify `archive` against its `<asset>.sha256` sidecar (best-effort fetch, strict compare). */
async function verifyChecksum(archive, sidecarUrl, asset) {
  let res;
  try {
    res = await fetch(sidecarUrl, { redirect: "follow" });
  } catch {
    res = null;
  }
  if (!res || !res.ok) {
    console.warn(`[kbuildx] no checksum sidecar for ${asset} — skipping verification.`);
    return;
  }
  // Sidecar is `shasum -a 256` output: "<hex>  <filename>". Take the first field.
  const expected = (await res.text()).trim().split(/\s+/)[0].toLowerCase();
  const actual = crypto.createHash("sha256").update(archive).digest("hex");
  if (expected !== actual) {
    throw new Error(
      `checksum mismatch for ${asset}:\n  expected ${expected}\n  actual   ${actual}`
    );
  }
  console.log(`[kbuildx] checksum OK (${actual.slice(0, 12)}…)`);
}

// Exported so the bin shim can self-heal: under `bunx` (and any install run
// with lifecycle scripts disabled) postinstall never runs, so the launcher
// calls install() on first use.
module.exports = { install: main };

if (require.main === module) {
  main().catch((err) => {
    console.error("\n[kbuildx] install failed:\n" + (err && err.message ? err.message : err));
    process.exit(1);
  });
}
