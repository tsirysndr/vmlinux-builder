module Test.Main where

import Prelude

import Data.Array as Array
import Data.Either (Either(..), isLeft)
import Data.Maybe (Maybe(..))
import Data.String (Pattern(..), contains, split)
import Data.String as String
import Data.Tuple (Tuple(..))
import Effect (Effect)
import Effect.Aff (Aff, launchAff_, throwError)
import Effect.Class (liftEffect)
import Effect.Console (log)
import Effect.Exception (error)
import VmlinuxBuilder.Config (ConfigSection(..), ConfigValue(..), Tristate(..))
import VmlinuxBuilder.Config as Config
import VmlinuxBuilder.DefaultConfig (defaultConfig)
import VmlinuxBuilder.Options (ConfigMode(..), Source(..), parseArgs)
import VmlinuxBuilder.Toml (TomlValue(..))
import VmlinuxBuilder.Toml as Toml
import VmlinuxBuilder.Version (KernelVersion(..), gitRef, parseKernelVersion, versionLabel)
import VmlinuxBuilder.Zfs (compareKernelLines, configHasZfs, configWithModules, configWithZfs, isCommit, kernelLine, parseZfsMeta, zfsCompatibility, zfsConfigureArgs, zfsEnabled, zfsForKernel, zfsSupports)

assert :: String -> Boolean -> Aff Unit
assert message ok = unless ok $ throwError $ error ("assertion failed: " <> message)

assertEq :: forall a. Eq a => Show a => String -> a -> a -> Aff Unit
assertEq message expected actual =
  unless (expected == actual) $ throwError $ error
    (message <> ": expected " <> show expected <> ", got " <> show actual)

simpleConfig :: String
simpleConfig = String.joinWith "\n"
  [ ""
  , "CONFIG_64BIT=y"
  , "CONFIG_SMP=y"
  , "CONFIG_NR_CPUS=64"
  , "# CONFIG_DEBUG is not set"
  , "CONFIG_LOCALVERSION=\"-test\""
  , ""
  ]

complexConfig :: String
complexConfig = String.joinWith "\n"
  [ "#"
  , "# Automatically generated file; DO NOT EDIT."
  , "# Linux/x86 6.6.100 Kernel Configuration"
  , "#"
  , "CONFIG_CC_VERSION_TEXT=\"gcc 13.3.0\""
  , "CONFIG_64BIT=y"
  , "#"
  , "# General setup"
  , "#"
  , "CONFIG_SMP=y"
  , "CONFIG_NR_CPUS=64"
  , "# CONFIG_EXPERT is not set"
  , "# end of General setup"
  , "#"
  , "# Security"
  , "#"
  , "CONFIG_SECURITY=y"
  , "CONFIG_SECURITY_SELINUX=y"
  , "# end of Security"
  ]

y :: Maybe ConfigValue
y = Just (Tristate Y)

main :: Effect Unit
main = launchAff_ do
  configTests
  tomlTests
  versionTests
  optionsTests
  zfsTests
  liftEffect (log "all tests passed")

-- ---------------------------------------------------------------------------

configTests :: Aff Unit
configTests = do
  let simple = Config.parse simpleConfig
  let complex = Config.parse complexConfig
  assertEq "simple config has five options" 5 (Config.flatSize simple.flatConfig)
  assert "complex config has sections" (not (Array.null complex.sections))
  assertEq "y values" y (Config.getValue "CONFIG_64BIT" simple)
  assertEq "numeric values" (Just (Numeric 64)) (Config.getValue "CONFIG_NR_CPUS" simple)
  assertEq "string values lose their quotes" (Just (Text "-test")) (Config.getValue "CONFIG_LOCALVERSION" simple)
  assertEq "disabled options" Nothing (Config.getValue "CONFIG_DEBUG" simple)
  assertEq "hex values" (Just (Numeric 16777216)) (Config.getValue "CONFIG_PHYSICAL_START" (Config.parse "CONFIG_PHYSICAL_START=0x1000000"))
  assertEq "empty config" 0 (Config.flatSize (Config.parse "").flatConfig)
  assertEq "quoted strings with spaces" (Just (Text "hello world")) (Config.getValue "CONFIG_TEXT" (Config.parse "CONFIG_TEXT=\"hello world\""))
  assertEq "quoted strings with special chars" (Just (Text "test#value")) (Config.getValue "CONFIG_TEXT" (Config.parse "CONFIG_TEXT=\"test#value\""))
  assertEq "empty string value" (Just (Text "")) (Config.getValue "CONFIG_TEXT" (Config.parse "CONFIG_TEXT=\"\""))
  assertEq "zero" (Just (Numeric 0)) (Config.getValue "CONFIG_NUM" (Config.parse "CONFIG_NUM=0"))
  assertEq "negative numbers" (Just (Numeric (-1))) (Config.getValue "CONFIG_NUM" (Config.parse "CONFIG_NUM=-1"))
  assertEq "module value" (Just (Tristate M)) (Config.getValue "CONFIG_MODULE" (Config.parse "CONFIG_MODULE=m"))
  assert "malformed input is skipped" (Config.flatSize (Config.parse "INVALID LINE\n###\n").flatConfig == 0)

  -- Sections nest as the kernel wrote them: two top-level sections here,
  -- not one with the other inside it.
  assertEq "top-level sections" [ "General setup", "Security" ] (complex.sections <#> \(ConfigSection s) -> s.name)
  assertEq "build info from the config" (Just (Just "gcc 13.3.0")) (complex.buildInfo <#> _.compiler)
  assertEq "the header's version" (Just "6.6.100") complex.version
  assertEq "no header, no version" Nothing simple.version

  -- Queries.
  assert "isEnabled on y" (Config.isEnabled "CONFIG_SMP" simple)
  assert "isEnabled on not set" (not (Config.isEnabled "CONFIG_DEBUG" simple))
  let processor = Config.extractProcessorConfig simple
  assert "processor smp" processor.smp
  assertEq "processor cpus" (Just 64) processor.nrCpus
  let security = Config.extractSecurityConfig complex
  assert "security flags" (security.security && security.selinux && not security.apparmor)

  -- Serialization.
  let serialized = Config.serialize Config.defaultSerializeOptions simple
  assert "serialize to config format" (contains (Pattern "CONFIG_64BIT=y") serialized)
  assert "serialize without header" (String.take 1 (Config.serialize Config.defaultSerializeOptions { addHeader = false } simple) /= "#")
  let
    sortedLines = Array.filter (\l -> String.take 7 l == "CONFIG_")
      (split (Pattern "\n") (Config.serialize Config.defaultSerializeOptions { sortKeys = true } simple))
  assertEq "serialize sorted keys" (Array.sort sortedLines) sortedLines
  let withSections = Config.serialize Config.defaultSerializeOptions complex
  assert "preserve section hierarchy" (contains (Pattern "# General setup") withSections && contains (Pattern "# Security") withSections)
  assert "flatten sections when disabled"
    (not (contains (Pattern "# end of") (Config.serialize Config.defaultSerializeOptions { preserveSections = false } complex)))
  assert "not set survives serialization" (contains (Pattern "# CONFIG_DEBUG is not set") serialized)

  -- Round trips.
  assertEq "round-trip simple config" simple.flatConfig (Config.parse serialized).flatConfig
  assertEq "round-trip complex config" (Config.flatSize complex.flatConfig) (Config.flatSize (Config.parse withSections).flatConfig)
  case Config.fromJson (Config.toJson simple) of
    Left err -> throwError (error ("round-trip via JSON: " <> err))
    Right back -> assertEq "round-trip via JSON" simple.flatConfig back.flatConfig
  case Config.fromJson "{\"sections\":[],\"flatConfig\":{\"CONFIG_SMP\":\"y\"}}" of
    Left err -> throwError (error ("deserialize from JSON: " <> err))
    Right c -> assertEq "deserialize from JSON" y (Config.getValue "CONFIG_SMP" c)
  assert "invalid JSON is an error" (isLeft (Config.fromJson "{"))

  -- Objects.
  let fromObj = Config.fromObject [ Tuple "CONFIG_SMP" (Just (Flag true)), Tuple "CONFIG_DEBUG" (Just (Flag false)), Tuple "OTHER" y ]
  assertEq "fromObject true is y" y (Config.getValue "CONFIG_SMP" fromObj)
  assertEq "fromObject false is not set" Nothing (Config.getValue "CONFIG_DEBUG" fromObj)
  assertEq "fromObject ignores non-CONFIG keys" 2 (Config.flatSize fromObj.flatConfig)
  assertEq "toObject keeps values" (Just y) (Array.head (Config.toObject false simple) <#> \(Tuple _ v) -> v)
  let booleans = Config.toObject true simple
  assertEq "toObject boolean style" (Just (Just (Flag true))) (Array.find (\(Tuple k _) -> k == "CONFIG_64BIT") booleans <#> \(Tuple _ v) -> v)
  assertEq "toObject boolean style, not set" (Just (Just (Flag false))) (Array.find (\(Tuple k _) -> k == "CONFIG_DEBUG") booleans <#> \(Tuple _ v) -> v)
  assertEq "round-trip via object" (Config.getValue "CONFIG_SMP" fromObj) (Config.getValue "CONFIG_SMP" (Config.fromObject (Config.toObject true fromObj)))

  -- Other forms.
  let yaml = Config.toYaml simple
  assert "yaml has config" (contains (Pattern "config:") yaml && contains (Pattern "CONFIG_SMP: \"y\"") yaml)
  assert "makefile" (contains (Pattern "SMP := y") (Config.toMakefile simple))
  let enabled = Config.toEnabledOnly simple
  assert "enabled only" (contains (Pattern "CONFIG_SMP=y") enabled && not (contains (Pattern "CONFIG_DEBUG") enabled))
  let shell = Config.toShellScript simple
  assert "shell script" (contains (Pattern "#!/bin/bash") shell && contains (Pattern "export CONFIG_SMP=y") shell)

  -- Diffs.
  let one = Config.fromObject [ Tuple "CONFIG_SMP" y ]
  let two = Config.fromObject [ Tuple "CONFIG_SMP" y, Tuple "CONFIG_DEBUG" y ]
  assert "detect added" (contains (Pattern "+ CONFIG_DEBUG=y") (Config.toDiff one two))
  assert "detect removed" (contains (Pattern "- CONFIG_DEBUG=y") (Config.toDiff two one))
  let changed = Config.toDiff (Config.fromObject [ Tuple "CONFIG_NR_CPUS" (Just (Numeric 64)) ]) (Config.fromObject [ Tuple "CONFIG_NR_CPUS" (Just (Numeric 128)) ])
  assert "detect changed" (contains (Pattern "- CONFIG_NR_CPUS=64") changed && contains (Pattern "+ CONFIG_NR_CPUS=128") changed)

  -- TOML.
  let withBuild = (Config.fromObject [ Tuple "CONFIG_SMP" y, Tuple "CONFIG_NR_CPUS" (Just (Numeric 8)) ]) { buildInfo = Just { compiler: Just "gcc 13.3.0", gccVersion: Nothing, buildSalt: Nothing } }
  let tomlText = Config.toToml withBuild
  assert "toToml has both tables" (contains (Pattern "[buildInfo]") tomlText && contains (Pattern "[config]") tomlText && contains (Pattern "gcc 13.3.0") tomlText)
  case Config.fromToml "CONFIG_SMP = \"y\"\nCONFIG_NR_CPUS = 32" of
    Left err -> throwError (error ("fromToml: " <> err))
    Right c -> do
      assertEq "fromToml flat keys" y (Config.getValue "CONFIG_SMP" c)
      assertEq "fromToml ints" (Just (Numeric 32)) (Config.getValue "CONFIG_NR_CPUS" c)
  assert "fromToml rejects invalid TOML" (isLeft (Config.fromToml "this is not = valid toml ="))
  case Config.fromToml tomlText of
    Left err -> throwError (error ("round-trip via TOML: " <> err))
    Right back -> do
      assertEq "round-trip via TOML" withBuild.flatConfig back.flatConfig
      assertEq "round-trip via TOML keeps build info" (Just (Just "gcc 13.3.0")) (back.buildInfo <#> _.compiler)

  -- The bundled default config is the one build.ts uses, read through Node.
  let bundled = Config.parse defaultConfig
  assert "default config is the real one" (Config.isEnabled "CONFIG_VIRTIO_BLK" bundled && Config.isEnabled "CONFIG_EFI_PARTITION" bundled)
  assert "default config has no modules" (not (Config.isEnabled "CONFIG_MODULES" bundled))

-- ---------------------------------------------------------------------------

tomlTests :: Aff Unit
tomlTests = do
  let
    doc =
      { root: [ Tuple "name" (TString "a \"quoted\" one"), Tuple "n" (TInt 3) ]
      , tables: [ Tuple "t" [ Tuple "ok" (TBool true), Tuple "f" (TFloat 1.5) ] ]
      }
  case Toml.parse (Toml.render doc) of
    Left err -> throwError (error ("toml round trip: " <> err))
    Right back -> assertEq "toml round trip" doc back
  assert "toml comments are ignored" (Toml.parse "# only\nx = 1 # trailing" == Right { root: [ Tuple "x" (TInt 1) ], tables: [] })
  assert "toml rejects arrays" (isLeft (Toml.parse "x = [1, 2]"))
  assert "toml rejects spaced keys" (isLeft (Toml.parse "a b = 1"))

-- ---------------------------------------------------------------------------

versionTests :: Aff Unit
versionTests = do
  assertEq "release" (Right (Release "6.16.2")) (parseKernelVersion "v6.16.2")
  assertEq "maintenance" (Right (Maintenance "6.16")) (parseKernelVersion "6.16.y")
  assertEq "major.minor" (Right (Release "7.2")) (parseKernelVersion "7.2")
  assert "garbage" (isLeft (parseKernelVersion "six"))
  assertEq "release ref" "v6.16.2" (gitRef (Release "6.16.2"))
  assertEq "maintenance ref" "linux-6.16.y" (gitRef (Maintenance "6.16"))
  assertEq "maintenance label" "6.16" (versionLabel (Maintenance "6.16"))


-- ---------------------------------------------------------------------------

optionsTests :: Aff Unit
optionsTests = do
  case parseArgs [ "7.2.15", "--no-zfs", "--initrd", "--uimage", "--uimage-comp=gzip", "--zfs-version", "zfs-2.4.4" ] of
    Left err -> throwError (error err)
    Right o -> do
      assertEq "upstream source" (Upstream (Release "7.2.15")) o.source
      assertEq "zfs off" (Just false) o.zfs
      assertEq "explicit zfs version" (Just "zfs-2.4.4") o.zfsVersion
      assert "initrd" o.initrd
      assertEq "uimage comp" (Just "gzip") (o.uimage <#> _.comp)
      assertEq "default config mode" DefaultConfig o.config
  case parseArgs [ "--repo", "https://example.com/linux", "--branch", "orange-pi 6.6/x" ] of
    Left err -> throwError (error err)
    Right o -> assertEq "custom label is filesystem-safe"
      (Custom { repo: "https://example.com/linux", ref: "orange-pi 6.6/x", label: "orange-pi-6.6-x" })
      o.source
  case parseArgs [ "6.6", "--defconfig", "sun60iw2_defconfig", "--merge-config", "x" ] of
    Left err -> throwError (error err)
    Right o -> assertEq "defconfig wins over merge-config" (BoardDefconfig "sun60iw2_defconfig") o.config
  assert "repo needs branch" (isLeft (parseArgs [ "--repo", "x" ]))
  assert "unknown flags are errors" (isLeft (parseArgs [ "7.2", "--bogus" ]))
  assert "missing value" (isLeft (parseArgs [ "7.2", "--zfs-version" ]))
  assert "a version is required" (isLeft (parseArgs []))

-- ---------------------------------------------------------------------------

zfsTests :: Aff Unit
zfsTests = do
  assert "upstream builds get zfs" (zfsEnabled Nothing { board: false })
  assert "board builds do not" (not (zfsEnabled Nothing { board: true }))
  assert "explicit wins" (zfsEnabled (Just true) { board: true } && not (zfsEnabled (Just false) { board: false }))
  assert "configure is kernel-only builtin"
    (zfsConfigureArgs "/k" == [ "--with-config=kernel", "--enable-linux-builtin", "--with-linux=/k", "--with-linux-obj=/k" ])
  let base = "CONFIG_64BIT=y\n# CONFIG_ZFS is not set\nCONFIG_ZFS_DEBUG=y\n\n"
  let withZfs = configWithZfs base
  assertEq "zfs on, debug off, once" "CONFIG_64BIT=y\nCONFIG_ZFS=y\n# CONFIG_ZFS_DEBUG is not set\n" withZfs
  assertEq "idempotent" withZfs (configWithZfs withZfs)
  assert "configHasZfs" (configHasZfs withZfs && not (configHasZfs base) && not (configHasZfs "CONFIG_ZFS=m\n"))
  assertEq "modules on" "CONFIG_64BIT=y\nCONFIG_MODULES=y\n" (configWithModules "CONFIG_64BIT=y\n# CONFIG_MODULES is not set\n")
  assertEq "modules already on" "CONFIG_MODULES=y\n" (configWithModules "CONFIG_MODULES=y\n")
  -- Kernel lines, META and the compatibility matrix.
  assertEq "kernel line of a release" (Just { major: 7, minor: 3 }) (kernelLine "7.3.5")
  assertEq "kernel line of a branch" (Just { major: 6, minor: 17 }) (kernelLine "6.17.y")
  assertEq "kernel line of an rc" (Just { major: 7, minor: 3 }) (kernelLine "v7.3.0-rc2")
  assertEq "not a kernel line" Nothing (kernelLine "orange-pi-6.6")
  let meta = parseZfsMeta "Meta:          1\nVersion:       2.4.4\nRelease:       1\nLinux-Maximum: 7.2\nLinux-Minimum: 4.18\n"
  assertEq "META version" (Just "2.4.4") meta.version
  assertEq "META maximum" (Just { major: 7, minor: 2 }) meta.linuxMaximum
  assert "2.4.4 supports 7.2 and 6.17" (zfsSupports meta { major: 7, minor: 2 } && zfsSupports meta { major: 6, minor: 17 })
  assert "2.4.4 refuses 7.3 (the first CI run) and 4.4" (not (zfsSupports meta { major: 7, minor: 3 }) && not (zfsSupports meta { major: 4, minor: 4 }))
  assert "a META without bounds is not refused" (zfsSupports (parseZfsMeta "Version: x\n") { major: 9, minor: 9 })
  assertEq "7.2 gets the release" (Just "zfs-2.4.4") (_.ref <$> zfsForKernel { major: 7, minor: 2 })
  assertEq "older kernels get the release" (Just "zfs-2.4.4") (_.ref <$> zfsForKernel { major: 6, minor: 17 })
  assertEq "7.3 is on master only" (Just true) ((\r -> r.development && isCommit r.ref) <$> zfsForKernel { major: 7, minor: 3 })
  assertEq "7.4 is uncovered: the build probes master" Nothing (_.ref <$> zfsForKernel { major: 7, minor: 4 })
  assertEq "too old for every row" Nothing (_.ref <$> zfsForKernel { major: 4, minor: 4 })
  assert "the matrix is newest-first" (Array.all identity (Array.zipWith (\a b -> compareKernelLines a.linuxMaximum b.linuxMaximum /= LT) zfsCompatibility (Array.drop 1 zfsCompatibility)))
  assert "every row has sane bounds and a ref of the right kind"
    (Array.all (\r -> compareKernelLines r.linuxMinimum r.linuxMaximum == LT && (if r.development then isCommit r.ref else String.take 4 r.ref == "zfs-")) zfsCompatibility)
