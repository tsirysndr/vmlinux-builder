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

/// The OpenZFS release copied into the tree. Bump it together with the kernel
/// versions this builds: each OpenZFS release names the newest kernel it
/// supports (2.4.4 reaches 7.2), and a kernel past that fails to compile.
export const DEFAULT_ZFS_VERSION = "zfs-2.4.4";

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
