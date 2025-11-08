#!/usr/bin/env -S deno run --allow-run --allow-read --allow-write --allow-env --allow-net
import chalk from "chalk";
import cfg from "./.default-config" with { type: "text" };

export * from "./config.ts";

async function run(cmd: string[]): Promise<void> {
  const process = new Deno.Command(cmd[0], {
    args: cmd.slice(1),
    stdout: "inherit",
    stderr: "inherit",
  });
  const { code } = await process.output();
  if (code !== 0) {
    Deno.exit(code);
  }
}

async function runQuiet(cmd: string[]): Promise<boolean> {
  const process = new Deno.Command(cmd[0], {
    args: cmd.slice(1),
    stdout: "null",
    stderr: "null",
  });
  const { code } = await process.output();
  return code === 0;
}

async function fileExists(path: string): Promise<boolean> {
  try {
    await Deno.stat(path);
    return true;
  } catch {
    return false;
  }
}

async function getMachineArch(): Promise<string> {
  const process = new Deno.Command("uname", {
    args: ["-m"],
    stdout: "piped",
  });
  const { stdout } = await process.output();
  return new TextDecoder().decode(stdout).trim();
}

async function getNproc(): Promise<string> {
  const process = new Deno.Command("nproc", {
    stdout: "piped",
  });
  const { stdout } = await process.output();
  return new TextDecoder().decode(stdout).trim();
}

const args = Deno.args;

if (args.length < 1) {
  console.log(chalk.yellow(`Usage: $0 <kernel-version>{.y|.Z}`));
  console.log("Example: ./build.sh 6.1 | 6.1.12 | 6.1.y | v6.1.12");
  Deno.exit(1);
}

const INPUT = args[0];
const NUM = INPUT.startsWith("v") ? INPUT.slice(1) : INPUT; // normalize by stripping optional leading 'v'

// Validate: X.Y, X.Y.Z, or X.Y.y
const versionRegex = /^[0-9]+\.[0-9]+(\.(y|[0-9]+))?$/;
if (!versionRegex.test(NUM)) {
  console.log(
    chalk.yellow(
      `Error: Invalid kernel version '${INPUT}'. Expected X.Y, X.Y.Z, or X.Y.y`
    )
  );
  console.log("Examples: 6.1 | 6.1.12 | 6.1.y | v6.1.12");
  Deno.exit(1);
}

console.log(`Building vmlinux for Linux kernel ${chalk.cyan(NUM)}`);

const hasAptGet = await runQuiet(["which", "apt-get"]);
if (hasAptGet) {
  try {
    await run([
      "sudo",
      "apt-get",
      "install",
      "-y",
      "git",
      "build-essential",
      "flex",
      "bison",
      "libncurses5-dev",
      "libssl-dev",
      "gcc",
      "bc",
      "libelf-dev",
      "pahole",
    ]);
  } catch {
    // Ignore errors
  }
}

const REPO_URL =
  "git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git";

// Decide ref: maintenance branch vs tag
let REF: string;
let VERSION: string;

if (NUM.endsWith(".y")) {
  REF = `linux-${NUM}`; // e.g. linux-6.16.y
  VERSION = NUM.slice(0, -2); // e.g. 6.16
} else {
  REF = `v${NUM}`; // e.g. v6.16.2 (ensure leading v)
  VERSION = NUM; // e.g. 6.16.2 (no leading v)
}

if (!(await fileExists("linux-stable"))) {
  // Clone directly at the desired ref (branch or tag)
  await run([
    "git",
    "clone",
    "--depth=1",
    "--branch",
    REF,
    REPO_URL,
    "linux-stable",
  ]);
} else {
  // Update existing checkout to the desired ref
  await run([
    "git",
    "-C",
    "linux-stable",
    "fetch",
    "--tags",
    "--force",
    "origin",
  ]);

  // Shallow-fetch the specific ref (works for both branches and tags)
  try {
    await run([
      "git",
      "-C",
      "linux-stable",
      "fetch",
      "--depth=1",
      "origin",
      `${REF}:${REF}`,
    ]);
  } catch {
    await run([
      "git",
      "-C",
      "linux-stable",
      "fetch",
      "origin",
      `${REF}:${REF}`,
    ]);
  }

  await run(["git", "-C", "linux-stable", "checkout", "-f", REF]);
}

if (!(await Deno.stat(".config").catch(() => false))) {
  console.log(
    chalk.yellow(
      "No .config file found in the current directory. Using default configuration."
    )
  );
  await Deno.writeTextFile(".config", cfg);
}

await Deno.copyFile(".config", "linux-stable/.config");

Deno.chdir("linux-stable");

const nproc = await getNproc();
const makeProcess = new Deno.Command("make", {
  args: ["vmlinux", `-j${nproc}`],
  stdin: "piped",
  stdout: "inherit",
  stderr: "inherit",
});

// Pipe empty input (equivalent to yes '' | make ... < /dev/null)
const yesProcess = new Deno.Command("yes", {
  args: [""],
  stdout: "piped",
});

const yes = yesProcess.spawn();
const make = makeProcess.spawn();

yes.stdout.pipeTo(make.stdin).catch((err) => {
  if (!err.message?.includes("Broken pipe")) {
    throw err;
  }
});

const { code: makeCode } = await make.status;

if (makeCode !== 0) {
  Deno.exit(makeCode);
}

// Rename vmlinux
const arch = await getMachineArch();
const VMLINUX = `vmlinux-${VERSION}`;
await Deno.rename("vmlinux", `${VMLINUX}.${arch}`);

console.log(chalk.green("vmlinux built successfully!"));
const cwd = Deno.cwd();
console.log(
  `You can find the vmlinux file in ${chalk.cyan(`${cwd}/${VMLINUX}.${arch}`)}`
);

Deno.exit(0);
