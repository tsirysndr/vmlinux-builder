#!/usr/bin/env -S deno run --allow-run --allow-read --allow-write --allow-env --allow-net
import _ from "@es-toolkit/es-toolkit/compat";
import chalk from "chalk";
import cfg from "./default-config.ts";

export * from "./config.ts";

async function run(cmd: string[]): Promise<void> {
  console.log(`Running: ${chalk.green(cmd.join(" "))}`);
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

const rawArgs = Deno.args;

// Parse optional flags:
//   --repo <url>      clone from a custom git repository instead of linux-stable
//   --branch <ref>    branch or tag to check out (required with --repo)
//   --version <label> label used to name the output vmlinux file (optional)
// Anything not matching a flag is treated as the positional kernel version.
let repoInput: string | undefined;
let branchInput: string | undefined;
let versionLabel: string | undefined;
const positional: string[] = [];

for (let i = 0; i < rawArgs.length; i++) {
  const arg = rawArgs[i];

  const takeValue = (name: string): string => {
    const eq = arg.indexOf("=");
    if (eq !== -1) return arg.slice(eq + 1);
    const next = rawArgs[i + 1];
    if (next === undefined) {
      console.log(chalk.yellow(`Error: missing value for ${name}`));
      Deno.exit(1);
    }
    i++;
    return next;
  };

  if (arg === "--repo" || arg.startsWith("--repo=")) {
    repoInput = takeValue("--repo");
  } else if (
    arg === "--branch" ||
    arg.startsWith("--branch=") ||
    arg === "--ref" ||
    arg.startsWith("--ref=")
  ) {
    branchInput = takeValue("--branch");
  } else if (arg === "--version" || arg.startsWith("--version=")) {
    versionLabel = takeValue("--version");
  } else {
    positional.push(arg);
  }
}

// Default upstream stable tree; overridden by --repo.
let REPO_URL =
  "git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git";
let REF: string;
let VERSION: string;

if (repoInput) {
  // Custom repository + branch/tag (e.g. an OrangePi/BSP kernel fork).
  if (!branchInput) {
    console.log(
      chalk.yellow("Error: --branch is required when --repo is provided")
    );
    console.log(
      "Example: ./build.ts --repo https://github.com/tsirysndr/linux-orangepi --branch orange-pi-6.6-sun60iw2"
    );
    Deno.exit(1);
  }

  REPO_URL = repoInput;
  REF = branchInput;
  // Name the artifact from --version, else the positional arg, else a
  // filesystem-safe form of the branch name.
  VERSION =
    versionLabel ??
    positional[0] ??
    branchInput.replace(/[^A-Za-z0-9._-]+/g, "-");

  console.log(
    `Building vmlinux from ${chalk.cyan(REPO_URL)} @ ${chalk.cyan(REF)}`
  );
} else {
  // Default path: build a version from the upstream linux-stable tree.
  if (positional.length < 1) {
    console.log(chalk.yellow(`Usage: $0 <kernel-version>{.y|.Z}`));
    console.log("Example: ./build.ts 6.1 | 6.1.12 | 6.1.y | v6.1.12");
    console.log(
      "Custom repo: ./build.ts --repo <git-url> --branch <branch> [--version <label>]"
    );
    Deno.exit(1);
  }

  const INPUT = positional[0];
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

  // Decide ref: maintenance branch vs tag
  if (NUM.endsWith(".y")) {
    REF = `linux-${NUM}`; // e.g. linux-6.16.y
    VERSION = NUM.slice(0, -2); // e.g. 6.16
  } else {
    REF = `v${NUM}`; // e.g. v6.16.2 (ensure leading v)
    VERSION = NUM; // e.g. 6.16.2 (no leading v)
  }
}

const hasAptGet = await runQuiet(["which", "apt-get"]);
const hasSudo = await runQuiet(["which", "sudo"]);
if (hasAptGet) {
  try {
    await run([
      ..._.compact([hasSudo ? "sudo" : null]),
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
  // Shallow-fetch the specific ref (works for both branches and tags)
  try {
    await run([
      "git",
      "-C",
      "linux-stable",
      "fetch",
      "--depth=1",
      "origin",
      REF,
    ]);
  } catch {
    await run(["git", "-C", "linux-stable", "fetch", "origin", REF]);
  }

  Deno.chdir("linux-stable");

  await run(["rm", "-rf", "Documentation/Kbuild"]);
  await run(["make", "mrproper"]);

  await run(["git", "checkout", "-f", REF]);

  Deno.chdir("..");
}

if (!(await Deno.stat(".config").catch(() => false))) {
  console.log(
    chalk.yellow(
      "No .config file found in the current directory. Using default configuration."
    )
  );
  await Deno.writeTextFile(".config", cfg);
}

Deno.chdir("linux-stable");

await Deno.copyFile("../.config", ".config");

await run(["make", "prepare"]);

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
