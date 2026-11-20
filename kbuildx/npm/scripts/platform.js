"use strict";

// Maps the running platform/arch onto the kbuildx release asset.
//
// Supported (what the kbuildx release workflow builds):
//   macOS  : arm64 only  (Apple Silicon)
//   Linux  : x64 + arm64
// Everything else — Windows, macOS on Intel, 32-bit, etc. — is unsupported.

const path = require("node:path");

// process.platform + process.arch -> the target triple in the asset name.
// Release assets are `kbuildx-v<version>-<triple>.tar.gz`, each holding one
// `kbuildx` executable; keep this in lockstep with the `kbuildx release`
// workflow's packaging step.
const TRIPLES = {
  "darwin arm64": "aarch64-apple-darwin",
  "linux x64": "x86_64-unknown-linux-gnu",
  "linux arm64": "aarch64-unknown-linux-gnu",
};

/** Human-readable list of what we do support, for error messages. */
const SUPPORTED = "macOS/arm64, Linux/x64, Linux/arm64";

/**
 * Resolve the release asset for the current platform and the package's
 * version, or throw a clear error naming what's supported.
 * Returns { platform, arch, triple, asset, tag }.
 */
function detect(version) {
  const platform = process.platform;
  const arch = process.arch;
  const triple = TRIPLES[platform + " " + arch];
  if (!triple) {
    throw new Error(
      `kbuildx: unsupported platform ${platform}/${arch}.\n` +
        `kbuildx ships prebuilt binaries only for: ${SUPPORTED}.\n` +
        (platform === "darwin"
          ? "macOS support is Apple Silicon (arm64) only — Intel Macs are not supported.\n"
          : "") +
        "Elsewhere, build from source: cargo build --release --locked\n" +
        "  https://github.com/tsirysndr/vmlinux-builder/tree/main/kbuildx"
    );
  }
  const tag = `kbuildx-v${version}`;
  return { platform, arch, triple, asset: `${tag}-${triple}.tar.gz`, tag };
}

/** Absolute path to where the downloaded native binary lives inside the package. */
function binaryPath() {
  return path.join(__dirname, "..", "binaries", "kbuildx");
}

module.exports = { detect, binaryPath, TRIPLES, SUPPORTED };
