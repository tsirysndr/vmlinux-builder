// Process and host helpers behind VmlinuxBuilder.Shell. Each returns a
// Promise; the PureScript side turns it into an Aff.
import { spawn } from "node:child_process";
import { rm } from "node:fs/promises";

function spawnWith(argv, options) {
  return new Promise((resolve, reject) => {
    const [cmd, ...args] = argv;
    const child = spawn(cmd, args, options);
    child.on("error", reject);
    child.on("close", (code, signal) => resolve(code ?? (signal ? 128 : 1)));
  });
}

export const runImpl = (argv) => () =>
  spawnWith(argv, { stdio: "inherit" });

export const runQuietImpl = (argv) => () =>
  spawnWith(argv, { stdio: "ignore" });

export const captureImpl = (argv) => () =>
  new Promise((resolve, reject) => {
    const [cmd, ...args] = argv;
    const child = spawn(cmd, args, { stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (out += chunk));
    child.on("error", reject);
    child.on("close", () => resolve(out));
  });

// `yes '' | cmd`: answers every prompt with the default. The pipe breaks
// when the command exits before `yes` does; that is the normal end.
export const runWithYesImpl = (argv) => () =>
  new Promise((resolve, reject) => {
    const [cmd, ...args] = argv;
    const child = spawn(cmd, args, { stdio: ["pipe", "inherit", "inherit"] });
    const yes = spawn("yes", [""], { stdio: ["ignore", "pipe", "ignore"] });
    yes.stdout.on("error", () => {});
    child.stdin.on("error", () => {});
    yes.stdout.pipe(child.stdin);
    child.on("error", reject);
    child.on("close", (code, signal) => {
      yes.kill();
      resolve(code ?? (signal ? 128 : 1));
    });
  });

export const fetchTextImpl = (url) => async () => {
  const resp = await fetch(url);
  if (!resp.ok) {
    throw new Error(`failed to fetch ${url} (${resp.status} ${resp.statusText})`);
  }
  return resp.text();
};

export const removeImpl = (path) => () => rm(path, { recursive: true, force: true });
