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

async function capture(cmd: string[]): Promise<string> {
  const process = new Deno.Command(cmd[0], {
    args: cmd.slice(1),
    stdout: "piped",
    stderr: "null",
  });
  const { stdout } = await process.output();
  return new TextDecoder().decode(stdout).trim();
}

const rawArgs = Deno.args;

// Parse optional flags:
//   --repo <url>          clone from a custom git repository instead of linux-stable
//   --branch <ref>        branch or tag to check out (required with --repo)
//   --version <label>     label used to name the output vmlinux file (optional)
//   --merge-config <src>  existing config to merge with the default config; the
//                         default config is appended last so it overrides on
//                         conflicts. <src> is an http(s) URL or a file resolved
//                         at the kernel tree root (e.g. .config or a defconfig).
//   --initrd              also generate an initrd (initrd.img) and, on arm64,
//                         a U-Boot uInitrd alongside the kernel.
//   --defconfig <name>    build a board/BSP kernel: run `make <name>` (e.g.
//                         sun60iw2_defconfig) as the base config so the board's
//                         essentials WIN, then layer our default config as a
//                         lower-priority fragment. CONFIG_WERROR is forced off.
// Anything not matching a flag is treated as the positional kernel version.
let repoInput: string | undefined;
let branchInput: string | undefined;
let versionLabel: string | undefined;
let mergeConfigInput: string | undefined;
let defconfigInput: string | undefined;
let genInitrd = false;
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
  } else if (
    arg === "--merge-config" ||
    arg.startsWith("--merge-config=") ||
    arg === "--config" ||
    arg.startsWith("--config=")
  ) {
    mergeConfigInput = takeValue("--merge-config");
  } else if (arg === "--defconfig" || arg.startsWith("--defconfig=")) {
    defconfigInput = takeValue("--defconfig");
  } else if (arg === "--initrd") {
    genInitrd = true;
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

if (defconfigInput) {
  console.log(
    chalk.magenta(
      `Board defconfig detected: ${chalk.cyan(
        defconfigInput
      )} — it will be used as the base config (board wins) with the default config layered underneath.`
    )
  );
}

if (mergeConfigInput) {
  console.log(
    chalk.magenta(
      `Merge config detected: ${chalk.cyan(
        mergeConfigInput
      )} — it will be merged with the default config (default overrides on conflicts).`
    )
  );
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

if (defconfigInput) {
  // Board/BSP path: the board defconfig is the base and WINS on conflicts,
  // our default config is layered underneath to fill in extras.
  Deno.chdir("linux-stable");

  console.log(
    `Using board defconfig ${chalk.cyan(
      defconfigInput
    )} as base (board essentials win); layering default config underneath.`
  );

  // Generate the board's full .config (e.g. `make sun60iw2_defconfig`).
  await run(["make", defconfigInput]);
  await Deno.copyFile(".config", "board.config");

  // Write our default config as a fragment and merge with the board config
  // LAST, so the board overrides our defaults on any conflicting symbol.
  await Deno.writeTextFile("default.config", cfg);
  await run([
    "scripts/kconfig/merge_config.sh",
    "-m",
    "default.config",
    "board.config",
  ]);

  // Older BSP trees often fail to build with modern GCC when warnings are
  // fatal; force CONFIG_WERROR off (appended last so olddefconfig honors it).
  // Any board-specific symbols (e.g. CONFIG_PM_DEVFREQ for the Allwinner DMC
  // devfreq driver) belong in the board's own defconfig, not here.
  const merged = await Deno.readTextFile(".config");
  await Deno.writeTextFile(".config", `${merged}\n# CONFIG_WERROR is not set\n`);

  // Normalize against this tree's Kconfig.
  await run(["make", "olddefconfig"]);
} else if (mergeConfigInput) {
  // Merge an existing config with the default config. We simply concatenate,
  // putting the default config LAST so it overrides the existing config on
  // conflicting symbols (kconfig keeps the last assignment when reading).
  Deno.chdir("linux-stable");

  let existing: string;
  if (/^https?:\/\//i.test(mergeConfigInput)) {
    console.log(
      `Fetching existing config from ${chalk.cyan(mergeConfigInput)}`
    );
    const resp = await fetch(mergeConfigInput);
    if (!resp.ok) {
      console.log(
        chalk.yellow(
          `Error: failed to fetch config (${resp.status} ${resp.statusText})`
        )
      );
      Deno.exit(1);
    }
    existing = await resp.text();
  } else {
    // Resolved at the kernel tree root (cwd is linux-stable), e.g. ".config"
    // or "arch/arm64/configs/sun60iw2_defconfig".
    console.log(
      `Merging existing config ${chalk.cyan(mergeConfigInput)} with default config (default overrides)`
    );
    existing = await Deno.readTextFile(mergeConfigInput);
  }

  await Deno.writeTextFile(".config", `${existing}\n${cfg}\n`);

  // Normalize the merged config against this tree's Kconfig (fills in new
  // symbols with their defaults, drops symbols that don't apply).
  await run(["make", "olddefconfig"]);
} else {
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
}

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

// Optionally generate an initrd (and a U-Boot uInitrd on arm64).
if (genInitrd) {
  console.log(chalk.magenta("Generating initrd..."));

  // Tools: mkinitramfs (initramfs-tools) and, for uInitrd, mkimage (u-boot-tools).
  if (hasAptGet) {
    try {
      await run([
        ..._.compact([hasSudo ? "sudo" : null]),
        "apt-get",
        "install",
        "-y",
        "initramfs-tools",
        "u-boot-tools",
      ]);
    } catch {
      // Ignore errors; the commands below will surface a clearer failure.
    }
  }

  // Resolve this tree's kernel release string (e.g. 6.6.98-sun60iw2).
  const krel = await capture(["make", "-s", "kernelrelease"]);

  // Install the freshly built modules so mkinitramfs can find them for $krel.
  await run([..._.compact([hasSudo ? "sudo" : null]), "make", "modules_install"]);

  // Build the initrd for this kernel release.
  const INITRD = `initrd.img-${krel}`;
  await run([
    ..._.compact([hasSudo ? "sudo" : null]),
    "mkinitramfs",
    "-o",
    INITRD,
    krel,
  ]);

  console.log(
    chalk.green(`initrd built: ${chalk.cyan(`${cwd}/${INITRD}`)}`)
  );

  // uInitrd is the U-Boot ramdisk form; only meaningful on arm64 boards.
  if (arch === "aarch64" || arch === "arm64") {
    await run([
      "mkimage",
      "-A",
      "arm64",
      "-O",
      "linux",
      "-T",
      "ramdisk",
      "-C",
      "gzip",
      "-n",
      `uInitrd ${krel}`,
      "-d",
      INITRD,
      "uInitrd",
    ]);
    console.log(
      chalk.green(`uInitrd built: ${chalk.cyan(`${cwd}/uInitrd`)}`)
    );
  }
}

Deno.exit(0);
