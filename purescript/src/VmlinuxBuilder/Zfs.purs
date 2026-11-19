-- | OpenZFS built into the kernel: the decisions and the `.config` edits,
-- | kept pure so they can be tested without a kernel tree. The build in
-- | `VmlinuxBuilder.Build` does the cloning and copying around them.
-- |
-- | ZFS lives out of tree; a vmlinux carries it only if its sources are
-- | copied in (OpenZFS's `copy-builtin`) and `CONFIG_ZFS=y` is set before
-- | the build. Every upstream build does this, pinned to one release, so a
-- | microVM booting the published kernel can run `zpool` with no module to
-- | load or match.
module VmlinuxBuilder.Zfs
  ( defaultZfsVersion
  , defaultZfsRepo
  , zfsEnabled
  , zfsConfigureArgs
  , configWithZfs
  , configWithModules
  , configHasZfs
  ) where

import Prelude

import Data.Array (filter)
import Data.Array as Array
import Data.Maybe (Maybe(..), fromMaybe)
import Data.String (Pattern(..), joinWith, split)
import Data.String.Regex (Regex, test)
import Data.String.Regex.Flags (multiline, noFlags)
import Data.String.Regex.Unsafe (unsafeRegex)

-- | The OpenZFS release copied into the tree. Each release names the newest
-- | kernel it supports (2.4.4 reaches 7.2); bump it with the kernels built.
defaultZfsVersion :: String
defaultZfsVersion = "zfs-2.4.4"

defaultZfsRepo :: String
defaultZfsRepo = "https://github.com/openzfs/zfs.git"

-- | Explicit `--zfs`/`--no-zfs` wins; otherwise upstream builds have ZFS and
-- | board builds do not — vendor trees were never built with OpenZFS's
-- | in-tree sources, and a board kernel rarely wants a filesystem this size.
zfsEnabled :: Maybe Boolean -> { board :: Boolean } -> Boolean
zfsEnabled explicit opts = fromMaybe (not opts.board) explicit

-- | `./configure` for a builtin, kernel-only build. `--with-config=kernel`
-- | skips the userland half and its library checks, which a vmlinux never
-- | links.
zfsConfigureArgs :: String -> Array String
zfsConfigureArgs kernelDir =
  [ "--with-config=kernel"
  , "--enable-linux-builtin"
  , "--with-linux=" <> kernelDir
  , "--with-linux-obj=" <> kernelDir
  ]

zfsLineRx :: Regex
zfsLineRx = unsafeRegex "^(CONFIG_ZFS(_DEBUG)?=|# CONFIG_ZFS(_DEBUG)? is not set)" noFlags

-- | ZFS on, debug off (it promotes warnings to errors and enables every
-- | ASSERT — for developing ZFS, not booting it). Idempotent: existing ZFS
-- | lines are replaced, not duplicated.
configWithZfs :: String -> String
configWithZfs config =
  joinWith "\n" (trimTrailing (filter (not <<< test zfsLineRx) (split (Pattern "\n") config)))
    <> "\nCONFIG_ZFS=y\n# CONFIG_ZFS_DEBUG is not set\n"

-- | Module support on. OpenZFS's configure learns the kernel API by
-- | compiling small external modules, which Kbuild refuses on a kernel
-- | configured without CONFIG_MODULES — how these kernels are configured.
-- | Switched on for that probe only; the saved config comes back after.
configWithModules :: String -> String
configWithModules config
  | test modulesOnRx config = config
  | otherwise =
      joinWith "\n" (trimTrailing (filter (_ /= "# CONFIG_MODULES is not set") (split (Pattern "\n") config)))
        <> "\nCONFIG_MODULES=y\n"

modulesOnRx :: Regex
modulesOnRx = unsafeRegex "^CONFIG_MODULES=y$" multiline

-- | `make olddefconfig` silently drops a symbol whose dependencies are off
-- | (ZFS needs BLOCK and EFI_PARTITION); a build checks rather than trusts.
configHasZfs :: String -> Boolean
configHasZfs = test (unsafeRegex "^CONFIG_ZFS=y$" multiline)

trimTrailing :: Array String -> Array String
trimTrailing lines = case Array.unsnoc lines of
  Just { init, last } | last == "" -> trimTrailing init
  _ -> lines
