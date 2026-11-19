import { assert, assertEquals } from "@std/assert";
import {
  compareKernelLines,
  configHasZfs,
  configWithModules,
  configWithZfs,
  isCommit,
  kernelLine,
  parseZfsMeta,
  ZFS_COMPATIBILITY,
  zfsConfigureArgs,
  zfsEnabled,
  zfsForKernel,
  zfsSupports,
} from "./zfs.ts";

Deno.test("upstream builds get ZFS by default, board builds opt in", () => {
  assertEquals(zfsEnabled(undefined, { board: false }), true);
  assertEquals(zfsEnabled(undefined, { board: true }), false);
  assertEquals(zfsEnabled(false, { board: false }), false);
  assertEquals(zfsEnabled(true, { board: true }), true);
});

Deno.test("kernel versions reduce to a line", () => {
  assertEquals(kernelLine("7.3.5"), { major: 7, minor: 3 });
  assertEquals(kernelLine("v7.2"), { major: 7, minor: 2 });
  assertEquals(kernelLine("6.17.y"), { major: 6, minor: 17 });
  assertEquals(kernelLine("7.3.0-rc2"), { major: 7, minor: 3 });
  assertEquals(kernelLine("orange-pi-6.6"), undefined);
});

const meta244 = "Meta:          1\nName:          zfs\nBranch:        1.0\nVersion:       2.4.4\nRelease:       1\nLinux-Maximum: 7.2\nLinux-Minimum: 4.18\n";

Deno.test("OpenZFS META says which kernels a tree builds against", () => {
  const meta = parseZfsMeta(meta244);
  assertEquals(meta.version, "2.4.4");
  assertEquals(meta.linuxMaximum, { major: 7, minor: 2 });
  assertEquals(meta.linuxMinimum, { major: 4, minor: 18 });
  assert(zfsSupports(meta, { major: 7, minor: 2 }));
  assert(zfsSupports(meta, { major: 6, minor: 17 }));
  assert(!zfsSupports(meta, { major: 7, minor: 3 }), "the error the first CI run hit");
  assert(!zfsSupports(meta, { major: 4, minor: 4 }));
  // A META without bounds (an odd fork) is not refused.
  assert(zfsSupports(parseZfsMeta("Version: x\n"), { major: 9, minor: 9 }));
});

Deno.test("a commit is told from a tag", () => {
  assert(isCommit("d1a5b599297754df52dc2a28ca984a012d91978d"));
  assert(!isCommit("zfs-2.4.4"));
  assert(!isCommit("master"));
});

Deno.test("the compatibility matrix picks a release when one covers the kernel, a snapshot only otherwise", () => {
  assertEquals(zfsForKernel({ major: 7, minor: 2 })?.ref, "zfs-2.4.4");
  assertEquals(zfsForKernel({ major: 6, minor: 17 })?.ref, "zfs-2.4.4");
  const row73 = zfsForKernel({ major: 7, minor: 3 })!;
  assert(row73.development && isCommit(row73.ref), "7.3 is on master only");
  assertEquals(zfsForKernel({ major: 7, minor: 4 }), undefined, "nothing covers 7.4: the build probes master");
  assertEquals(zfsForKernel({ major: 4, minor: 4 }), undefined, "too old for every row");
});

Deno.test("the matrix is newest-first with sane bounds, so the first covering release is the newest", () => {
  for (let i = 0; i < ZFS_COMPATIBILITY.length; i++) {
    const row = ZFS_COMPATIBILITY[i];
    assert(compareKernelLines(row.linuxMinimum, row.linuxMaximum) < 0, row.ref);
    assert(row.development ? isCommit(row.ref) : /^zfs-\d+\.\d+\.\d+$/.test(row.ref), row.ref);
    if (i > 0) {
      assert(compareKernelLines(ZFS_COMPATIBILITY[i - 1].linuxMaximum, row.linuxMaximum) >= 0, "newest first");
    }
  }
});
