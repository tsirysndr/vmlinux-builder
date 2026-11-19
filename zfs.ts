// OpenZFS built into the kernel.
//
// ZFS lives out of tree, so a vmlinux carries it only if its sources are
// copied into the kernel tree before the build (OpenZFS's own
// `copy-builtin` does that) and CONFIG_ZFS is switched on. Every default
// build does this, pinned to one OpenZFS release, so a microVM booting the
// published kernel can import and create pools with nothing but `zpool` in
// its image. The pieces here are the pure ones — what the build decides and
// what it writes — kept apart from build.ts so they can be tested without
// cloning a kernel.

/// A kernel line, `7.3` of `7.3.5`: what OpenZFS states its support against.
export interface KernelLine {
  major: number;
  minor: number;
}

/// What an OpenZFS tree says about the kernels it builds against, from its
/// `META` file (`Linux-Maximum: 7.3`, `Linux-Minimum: 4.18`).
export interface ZfsMeta {
  version: string | undefined;
  linuxMinimum: KernelLine | undefined;
  linuxMaximum: KernelLine | undefined;
}

/// `7.3`, `7.3.5` or `7.3.y` as a kernel line; `undefined` for anything else.
export function kernelLine(version: string): KernelLine | undefined {
  const m = version.trim().replace(/^v/, "").match(/^(\d+)\.(\d+)(?:\.(?:y|\d+))?(?:-.*)?$/);
  return m ? { major: Number(m[1]), minor: Number(m[2]) } : undefined;
}

export function compareKernelLines(a: KernelLine, b: KernelLine): number {
  return a.major !== b.major ? a.major - b.major : a.minor - b.minor;
}

/// Parse an OpenZFS `META` file.
export function parseZfsMeta(text: string): ZfsMeta {
  const field = (name: string) =>
    text.split("\n").find((l) => l.startsWith(name + ":"))?.slice(name.length + 1).trim();
  const line = (v: string | undefined) => (v === undefined ? undefined : kernelLine(v));
  return {
    version: field("Version"),
    linuxMinimum: line(field("Linux-Minimum")),
    linuxMaximum: line(field("Linux-Maximum")),
  };
}

/// Whether a tree with this META builds against the kernel. OpenZFS's
/// configure enforces exactly these two bounds.
export function zfsSupports(meta: ZfsMeta, kernel: KernelLine): boolean {
  const aboveMin = meta.linuxMinimum === undefined || compareKernelLines(kernel, meta.linuxMinimum) >= 0;
  const belowMax = meta.linuxMaximum === undefined || compareKernelLines(kernel, meta.linuxMaximum) <= 0;
  return aboveMin && belowMax;
}

/// A full git commit id, fetched by hash rather than cloned by branch.
export function isCommit(ref: string): boolean {
  return /^[0-9a-f]{40}$/.test(ref);
}

/// The branch tried when no row of the matrix covers the kernel: OpenZFS
/// lands support for a new kernel on master first and backports it later.
export const ZFS_DEVELOPMENT_BRANCH = "master";

/// One row of the compatibility matrix: an OpenZFS ref and the kernel lines
/// it builds against, as its own META states them.
export interface ZfsCompatibility {
  /// Git tag, or a commit for a development snapshot.
  ref: string;
  /// OpenZFS's own version string, for the log.
  version: string;
  linuxMinimum: KernelLine;
  linuxMaximum: KernelLine;
  /// A snapshot of master rather than a release.
  development: boolean;
}

/// The OpenZFS compatibility matrix, newest OpenZFS first. Each row's bounds
/// are what that tree's META says (`Linux-Maximum`/`Linux-Minimum`), which
/// configure enforces to the letter. Add a row when a release ships with a
/// new `Linux-Maximum`; a development row can go once a release covers its
/// kernel. The checked-out tree's META is verified against the kernel after
/// checkout regardless, so a wrong row fails at once with both numbers.
export const ZFS_COMPATIBILITY: ReadonlyArray<ZfsCompatibility> = [
  {
    // openzfs/zfs master of 2026-11-19 ("CI: Temporarily ignore Lustre build
    // result"): Linux 7.3 support is on master only; the backport to 2.4.5
    // (openzfs/zfs#19838) is still open.
    ref: "d1a5b599297754df52dc2a28ca984a012d91978d",
    version: "2.4.99",
    linuxMinimum: { major: 4, minor: 18 },
    linuxMaximum: { major: 7, minor: 3 },
    development: true,
  },
  {
    ref: "zfs-2.4.4",
    version: "2.4.4",
    linuxMinimum: { major: 4, minor: 18 },
    linuxMaximum: { major: 7, minor: 2 },
    development: false,
  },
  {
    ref: "zfs-2.3.9",
    version: "2.3.9",
    linuxMinimum: { major: 4, minor: 18 },
    linuxMaximum: { major: 7, minor: 2 },
    development: false,
  },
];

/// The row to build into a kernel: a release that covers it, the newest
/// first; a development snapshot only when no release does. `undefined`
/// when no row covers the kernel line.
export function zfsForKernel(kernel: KernelLine): ZfsCompatibility | undefined {
  const covers = (row: ZfsCompatibility) =>
    compareKernelLines(kernel, row.linuxMinimum) >= 0 && compareKernelLines(kernel, row.linuxMaximum) <= 0;
  return ZFS_COMPATIBILITY.find((r) => !r.development && covers(r)) ?? ZFS_COMPATIBILITY.find(covers);
}

export const DEFAULT_ZFS_REPO = "https://github.com/openzfs/zfs.git";

/// Whether this build gets ZFS. Explicit `--zfs`/`--no-zfs` wins; otherwise
/// the upstream linux-stable builds have it and board/BSP builds (`--repo`,
/// `--defconfig`) do not — their vendor trees and defconfigs were never built
/// with OpenZFS's in-tree sources, and a board kernel rarely wants a
/// filesystem of this size.
export function zfsEnabled(
  explicit: boolean | undefined,
  opts: { board: boolean },
): boolean {
  return explicit ?? !opts.board;
}

/// `./configure` arguments for a builtin, kernel-only build against `kernelDir`.
/// `--with-config=kernel` skips the userland half (and its library checks:
/// libtirpc, libudev, python, …), which a vmlinux never links.
export function zfsConfigureArgs(kernelDir: string): string[] {
  return [
    "--with-config=kernel",
    "--enable-linux-builtin",
    `--with-linux=${kernelDir}`,
    `--with-linux-obj=${kernelDir}`,
  ];
}

/// A `.config` with ZFS switched on. Replaces an existing ZFS line rather than
/// appending a duplicate, and leaves ZFS_DEBUG off: it promotes warnings to
/// errors and enables every ASSERT, which is for developing ZFS, not booting it.
export function configWithZfs(config: string): string {
  const lines = config
    .split("\n")
    .filter((l) => !/^(CONFIG_ZFS(_DEBUG)?=|# CONFIG_ZFS(_DEBUG)? is not set)/.test(l));
  while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return `${lines.join("\n")}\nCONFIG_ZFS=y\n# CONFIG_ZFS_DEBUG is not set\n`;
}

/// The same `.config` with loadable-module support on. OpenZFS's configure
/// learns the kernel's API by compiling small external modules against the
/// tree, and Kbuild refuses an external-module build on a kernel configured
/// without CONFIG_MODULES — which is how these microVM kernels are
/// configured. Modules are switched on only for that probe; the original
/// config comes back for the real build.
export function configWithModules(config: string): string {
  if (/^CONFIG_MODULES=y$/m.test(config)) return config;
  const lines = config.split("\n").filter((l) => l !== "# CONFIG_MODULES is not set");
  while (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return `${lines.join("\n")}\nCONFIG_MODULES=y\n`;
}

/// Whether a normalized `.config` kept ZFS. `make olddefconfig` silently drops
/// a symbol whose dependencies are off (ZFS needs BLOCK and EFI_PARTITION), so
/// a build has to check rather than trust what it wrote.
export function configHasZfs(config: string): boolean {
  return /^CONFIG_ZFS=y$/m.test(config);
}
