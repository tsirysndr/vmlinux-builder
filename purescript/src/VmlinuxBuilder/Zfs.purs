module VmlinuxBuilder.Zfs
  ( KernelLine
  , kernelLine
  , compareKernelLines
  , ZfsMeta
  , parseZfsMeta
  , zfsSupports
  , ZfsCompatibility
  , zfsCompatibility
  , zfsForKernel
  , developmentBranch
  , defaultZfsRepo
  , isCommit
  , zfsEnabled
  , zfsConfigureArgs
  , configWithZfs
  , configWithModules
  , configHasZfs
  ) where

import Prelude

import Data.Array (filter)
import Data.Array as Array
import Data.Int as Int
import Data.Maybe (Maybe(..), fromMaybe)
import Data.String (Pattern(..), joinWith, split, stripPrefix, trim)
import Data.String.Regex (Regex, match, test)
import Data.String.Regex.Flags (multiline, noFlags)
import Data.String.Regex.Unsafe (unsafeRegex)

-- | A kernel line, `7.3` of `7.3.5`: what OpenZFS states its support against.
type KernelLine = { major :: Int, minor :: Int }

kernelLineRx :: Regex
kernelLineRx = unsafeRegex "^(\\d+)\\.(\\d+)(?:\\.(?:y|\\d+))?(?:-.*)?$" noFlags

-- | `7.3`, `7.3.5`, `7.3.y` or `7.3.0-rc2` as a kernel line.
kernelLine :: String -> Maybe KernelLine
kernelLine version = do
  let v = fromMaybe (trim version) (stripPrefix (Pattern "v") (trim version))
  groups <- Array.fromFoldable <$> match kernelLineRx v
  major <- join (Array.index groups 1) >>= Int.fromString
  minor <- join (Array.index groups 2) >>= Int.fromString
  pure { major, minor }

compareKernelLines :: KernelLine -> KernelLine -> Ordering
compareKernelLines a b = case compare a.major b.major of
  EQ -> compare a.minor b.minor
  other -> other

-- | What an OpenZFS tree says about the kernels it builds against, from its
-- | `META` file (`Linux-Maximum: 7.3`, `Linux-Minimum: 4.18`).
type ZfsMeta =
  { version :: Maybe String
  , linuxMinimum :: Maybe KernelLine
  , linuxMaximum :: Maybe KernelLine
  }

parseZfsMeta :: String -> ZfsMeta
parseZfsMeta text =
  { version: field "Version"
  , linuxMinimum: field "Linux-Minimum" >>= kernelLine
  , linuxMaximum: field "Linux-Maximum" >>= kernelLine
  }
  where
  field name = Array.findMap (\l -> trim <$> stripPrefix (Pattern (name <> ":")) l) (split (Pattern "\n") text)

-- | Whether a tree with this META builds against the kernel: the two bounds
-- | OpenZFS's configure enforces.
zfsSupports :: ZfsMeta -> KernelLine -> Boolean
zfsSupports meta k =
  maybe' true (\lo -> compareKernelLines k lo /= LT) meta.linuxMinimum
    && maybe' true (\hi -> compareKernelLines k hi /= GT) meta.linuxMaximum
  where
  maybe' d f = case _ of
    Just x -> f x
    Nothing -> d

-- | One row of the compatibility matrix: an OpenZFS ref and the kernel lines
-- | it builds against, as its own META states them. `development` marks a
-- | snapshot of master rather than a release.
type ZfsCompatibility =
  { ref :: String
  , version :: String
  , linuxMinimum :: KernelLine
  , linuxMaximum :: KernelLine
  , development :: Boolean
  }

-- | The OpenZFS compatibility matrix, newest OpenZFS first. Each row's bounds
-- | are what that tree's META says, which configure enforces to the letter.
-- | Add a row when a release ships with a new `Linux-Maximum`; a development
-- | row can go once a release covers its kernel. The checked-out tree's META
-- | is verified against the kernel after checkout regardless, so a wrong row
-- | fails at once with both numbers.
zfsCompatibility :: Array ZfsCompatibility
zfsCompatibility =
  [ -- openzfs/zfs master of 2026-11-19: Linux 7.3 support is on master only;
    -- the backport to 2.4.5 (openzfs/zfs#19838) is still open.
    { ref: "d1a5b599297754df52dc2a28ca984a012d91978d"
    , version: "2.4.99"
    , linuxMinimum: { major: 4, minor: 18 }
    , linuxMaximum: { major: 7, minor: 3 }
    , development: true
    }
  , { ref: "zfs-2.4.4", version: "2.4.4", linuxMinimum: { major: 4, minor: 18 }, linuxMaximum: { major: 7, minor: 2 }, development: false }
  , { ref: "zfs-2.3.9", version: "2.3.9", linuxMinimum: { major: 4, minor: 18 }, linuxMaximum: { major: 7, minor: 2 }, development: false }
  ]

-- | The row to build into a kernel: a release that covers it, the newest
-- | first; a development snapshot only when no release does.
zfsForKernel :: KernelLine -> Maybe ZfsCompatibility
zfsForKernel k = case Array.find (\r -> not r.development && covers r) zfsCompatibility of
  Just row -> Just row
  Nothing -> Array.find covers zfsCompatibility
  where
  covers r = compareKernelLines k r.linuxMinimum /= LT && compareKernelLines k r.linuxMaximum /= GT

-- | The branch tried when no row covers the kernel: OpenZFS lands support
-- | for a new kernel on master first and backports it later.
developmentBranch :: String
developmentBranch = "master"

defaultZfsRepo :: String
defaultZfsRepo = "https://github.com/openzfs/zfs.git"

-- | A full git commit id: fetched by hash rather than cloned by branch.
isCommit :: String -> Boolean
isCommit = test (unsafeRegex "^[0-9a-f]{40}$" noFlags)

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
