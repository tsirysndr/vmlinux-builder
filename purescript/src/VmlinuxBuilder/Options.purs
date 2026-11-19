-- | The command line, as data. Every flag `build.ts` accepts is here with
-- | the same spelling, parsed into a record the build reads without
-- | re-checking anything.
module VmlinuxBuilder.Options
  ( Source(..)
  , ConfigMode(..)
  , UimageOptions
  , defaultUimage
  , Options
  , parseArgs
  , usage
  , isBoardBuild
  , outputLabel
  ) where

import Prelude

import Control.Alt ((<|>))
import Data.Array as Array
import Data.Either (Either(..))
import Data.List (List(..), (:))
import Data.List as List
import Data.Maybe (Maybe(..), fromMaybe, isJust)
import Data.String (Pattern(..), joinWith, stripPrefix)
import Data.String.Regex (replace)
import Data.String.Regex.Flags (global)
import Data.String.Regex.Unsafe (unsafeRegex)
import VmlinuxBuilder.Version (KernelVersion, parseKernelVersion, versionLabel)
import VmlinuxBuilder.Zfs as Zfs

-- | Where the kernel comes from: an upstream linux-stable version, or a
-- | custom repository at a branch or tag (a board/BSP tree).
data Source
  = Upstream KernelVersion
  | Custom { repo :: String, ref :: String, label :: String }

derive instance Eq Source

instance Show Source where
  show (Upstream v) = "(Upstream " <> show v <> ")"
  show (Custom c) = "(Custom " <> show c <> ")"

-- | How `.config` is produced: the bundled default; the default with an
-- | existing config merged on top; or a board defconfig with the default
-- | layered underneath.
data ConfigMode
  = DefaultConfig
  | MergeConfig String
  | BoardDefconfig String

derive instance Eq ConfigMode

instance Show ConfigMode where
  show DefaultConfig = "DefaultConfig"
  show (MergeConfig s) = "(MergeConfig " <> show s <> ")"
  show (BoardDefconfig s) = "(BoardDefconfig " <> show s <> ")"

-- | `mkimage` parameters for an arm64 U-Boot image.
type UimageOptions =
  { arch :: String
  , os :: String
  , type_ :: String
  , comp :: String
  , load :: String
  , entry :: String
  , name :: Maybe String
  }

defaultUimage :: UimageOptions
defaultUimage =
  { arch: "arm"
  , os: "linux"
  , type_: "kernel"
  , comp: "none"
  , load: "0x41000000"
  , entry: "0x41000000"
  , name: Nothing
  }

type Options =
  { source :: Source
  , config :: ConfigMode
  , initrd :: Boolean
  , modules :: Boolean
  , uimage :: Maybe UimageOptions
  -- | `--zfs`/`--no-zfs`; `Nothing` leaves it to the build's default.
  , zfs :: Maybe Boolean
  , zfsVersion :: String
  , zfsRepo :: String
  }

-- | A board/BSP build: a custom tree or a board defconfig.
isBoardBuild :: Options -> Boolean
isBoardBuild o = case o.source, o.config of
  Custom _, _ -> true
  _, BoardDefconfig _ -> true
  _, _ -> false

-- | The `<label>` in `vmlinux-<label>.<arch>`.
outputLabel :: Options -> String
outputLabel o = case o.source of
  Upstream v -> versionLabel v
  Custom c -> c.label

usage :: String
usage = joinWith "\n"
  [ "Usage: vmlinux-builder <kernel-version>{.y|.Z} [flags]"
  , "       vmlinux-builder --repo <git-url> --branch <ref> [--version <label>] [flags]"
  , ""
  , "  --merge-config <src>  merge an existing config over the default (file at the tree root, or URL)"
  , "  --defconfig <name>    board build: `make <name>` as the base, default config layered under it"
  , "  --initrd              also generate an initrd (and a uInitrd on arm64)"
  , "  --modules             also build and archive lib/modules (when CONFIG_MODULES=y)"
  , "  --uimage [--uimage-arch|os|type|comp|load|entry|name <v>]  wrap the arm64 Image for U-Boot"
  , "  --zfs | --no-zfs      build OpenZFS in (default: on for upstream builds, off for board builds)"
  , "  --zfs-version <tag>   OpenZFS tag (default: " <> Zfs.defaultZfsVersion <> ")"
  , "  --zfs-repo <url>      OpenZFS repository (default: " <> Zfs.defaultZfsRepo <> ")"
  ]

-- | Flags as they are seen, before the source is resolved.
type Raw =
  { repo :: Maybe String
  , branch :: Maybe String
  , version :: Maybe String
  , mergeConfig :: Maybe String
  , defconfig :: Maybe String
  , initrd :: Boolean
  , modules :: Boolean
  , uimage :: Boolean
  , uimageOpts :: UimageOptions
  , zfs :: Maybe Boolean
  , zfsVersion :: String
  , zfsRepo :: String
  , positional :: Array String
  }

emptyRaw :: Raw
emptyRaw =
  { repo: Nothing
  , branch: Nothing
  , version: Nothing
  , mergeConfig: Nothing
  , defconfig: Nothing
  , initrd: false
  , modules: false
  , uimage: false
  , uimageOpts: defaultUimage
  , zfs: Nothing
  , zfsVersion: Zfs.defaultZfsVersion
  , zfsRepo: Zfs.defaultZfsRepo
  , positional: []
  }

-- | Parse the arguments after the program name.
parseArgs :: Array String -> Either String Options
parseArgs args = go emptyRaw (List.fromFoldable args) >>= resolve
  where
  -- A valued flag is `--flag value` or `--flag=value`.
  go :: Raw -> List String -> Either String Raw
  go raw Nil = Right raw
  go raw (arg : rest) = case flagValue arg rest of
    Just (Value name value rest') -> setValue raw name value >>= \raw' -> go raw' rest'
    Just (Missing name) -> Left ("missing value for " <> name)
    Nothing -> case arg of
      "--initrd" -> go raw { initrd = true } rest
      "--modules" -> go raw { modules = true } rest
      "--uimage" -> go raw { uimage = true } rest
      "--zfs" -> go raw { zfs = Just true } rest
      "--no-zfs" -> go raw { zfs = Just false } rest
      _ | isJust (stripPrefix (Pattern "--") arg) -> Left ("unknown flag " <> arg)
      _ -> go raw { positional = Array.snoc raw.positional arg } rest

  flagValue arg rest = case Array.find (\n -> arg == n || isJust (stripPrefix (Pattern (n <> "=")) arg)) valued of
    Nothing -> Nothing
    Just name -> case stripPrefix (Pattern (name <> "=")) arg of
      Just v -> Just (Value name v rest)
      Nothing -> case rest of
        v : rest' -> Just (Value name v rest')
        Nil -> Just (Missing name)

  valued =
    [ "--repo", "--branch", "--ref", "--version", "--merge-config", "--config", "--defconfig"
    , "--uimage-arch", "--uimage-os", "--uimage-type", "--uimage-comp", "--uimage-load"
    , "--uimage-entry", "--uimage-name", "--zfs-version", "--zfs-repo"
    ]

  setValue raw name v = Right case name of
    "--repo" -> raw { repo = Just v }
    "--branch" -> raw { branch = Just v }
    "--ref" -> raw { branch = Just v }
    "--version" -> raw { version = Just v }
    "--merge-config" -> raw { mergeConfig = Just v }
    "--config" -> raw { mergeConfig = Just v }
    "--defconfig" -> raw { defconfig = Just v }
    "--uimage-arch" -> raw { uimageOpts = raw.uimageOpts { arch = v } }
    "--uimage-os" -> raw { uimageOpts = raw.uimageOpts { os = v } }
    "--uimage-type" -> raw { uimageOpts = raw.uimageOpts { type_ = v } }
    "--uimage-comp" -> raw { uimageOpts = raw.uimageOpts { comp = v } }
    "--uimage-load" -> raw { uimageOpts = raw.uimageOpts { load = v } }
    "--uimage-entry" -> raw { uimageOpts = raw.uimageOpts { entry = v } }
    "--uimage-name" -> raw { uimageOpts = raw.uimageOpts { name = Just v } }
    "--zfs-version" -> raw { zfsVersion = v }
    "--zfs-repo" -> raw { zfsRepo = v }
    _ -> raw

  resolve raw = do
    source <- case raw.repo of
      Just repo -> case raw.branch of
        Nothing -> Left "--branch is required when --repo is provided"
        Just ref -> Right $ Custom
          { repo
          , ref
          , label: fromMaybe (safeLabel ref) (raw.version <|> Array.head raw.positional)
          }
      Nothing -> case Array.head raw.positional of
        Nothing -> Left "a kernel version is required"
        Just v -> Upstream <$> parseKernelVersion v
    config <- case raw.defconfig, raw.mergeConfig of
      Just d, _ -> Right (BoardDefconfig d)
      Nothing, Just m -> Right (MergeConfig m)
      Nothing, Nothing -> Right DefaultConfig
    pure
      { source
      , config
      , initrd: raw.initrd
      , modules: raw.modules
      , uimage: if raw.uimage then Just raw.uimageOpts else Nothing
      , zfs: raw.zfs
      , zfsVersion: raw.zfsVersion
      , zfsRepo: raw.zfsRepo
      }

  -- A filesystem-safe form of a branch name, for the artifact.
  safeLabel = replace (unsafeRegex "[^A-Za-z0-9._-]+" global) "-"

data Flag
  = Value String String (List String)
  | Missing String
