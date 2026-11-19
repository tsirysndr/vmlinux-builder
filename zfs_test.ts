import { assert, assertEquals } from "@std/assert";
import {
  configHasZfs,
  configWithModules,
  configWithZfs,
  DEFAULT_ZFS_VERSION,
  zfsConfigureArgs,
  zfsEnabled,
} from "./zfs.ts";

Deno.test("upstream builds get ZFS by default, board builds opt in", () => {
  assertEquals(zfsEnabled(undefined, { board: false }), true);
  assertEquals(zfsEnabled(undefined, { board: true }), false);
  assertEquals(zfsEnabled(false, { board: false }), false);
  assertEquals(zfsEnabled(true, { board: true }), true);
});

Deno.test("the pinned OpenZFS release is a tag name", () => {
  assert(/^zfs-\d+\.\d+\.\d+$/.test(DEFAULT_ZFS_VERSION));
});

Deno.test("configure is kernel-only, builtin, against the given tree", () => {
  const args = zfsConfigureArgs("/work/linux-stable");
  assert(args.includes("--with-config=kernel"));
  assert(args.includes("--enable-linux-builtin"));
  assert(args.includes("--with-linux=/work/linux-stable"));
  assert(args.includes("--with-linux-obj=/work/linux-stable"));
});

Deno.test("configWithModules turns modules on only when they are off", () => {
  assertEquals(
    configWithModules("CONFIG_64BIT=y\n# CONFIG_MODULES is not set\n"),
    "CONFIG_64BIT=y\nCONFIG_MODULES=y\n",
  );
  const already = "CONFIG_MODULES=y\nCONFIG_64BIT=y\n";
  assertEquals(configWithModules(already), already);
});

Deno.test("configWithZfs enables ZFS once, debug off, without duplicates", () => {
  const base = "CONFIG_64BIT=y\n# CONFIG_ZFS is not set\nCONFIG_ZFS_DEBUG=y\n\n";
  const out = configWithZfs(base);
  assertEquals(out, "CONFIG_64BIT=y\nCONFIG_ZFS=y\n# CONFIG_ZFS_DEBUG is not set\n");
  // Applying it again changes nothing: a reused tree does not accumulate lines.
  assertEquals(configWithZfs(out), out);
  assert(configHasZfs(out));
  assert(!configHasZfs(base));
  assert(!configHasZfs("CONFIG_ZFS=m\n"));
});
