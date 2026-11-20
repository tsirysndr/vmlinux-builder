#!/usr/bin/env node
"use strict";

// Thin launcher: exec the platform-native `kbuildx` binary that postinstall
// downloaded into ./binaries/, forwarding argv, stdio and the exit status.

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { binaryPath } = require("../scripts/platform.js");

async function main() {
  const bin = binaryPath();

  // Self-heal: `bunx` (and any install with lifecycle scripts disabled) never
  // runs postinstall, so the binary won't be here. Download it on first use.
  if (!fs.existsSync(bin)) {
    try {
      const { install } = require("../scripts/postinstall.js");
      await install();
    } catch (err) {
      console.error(
        "[kbuildx] native binary not found and on-demand download failed:\n" +
          "  " +
          (err && err.message ? err.message : err) +
          "\nRun the install script manually:\n" +
          "  node " +
          path.join(__dirname, "..", "scripts", "postinstall.js")
      );
      process.exit(1);
    }
    if (!fs.existsSync(bin)) {
      console.error("[kbuildx] native binary still missing after download at " + bin);
      process.exit(1);
    }
  }

  const res = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  if (res.error) {
    console.error("[kbuildx] failed to launch: " + res.error.message);
    process.exit(1);
  }
  if (res.signal) {
    process.exit(1);
  }
  process.exit(res.status === null ? 1 : res.status);
}

main();
