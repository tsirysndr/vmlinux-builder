-- | The build, step by step, as `build.ts` does it: install the toolchain,
-- | check the kernel out, produce `.config`, prepare, build OpenZFS in,
-- | `make vmlinux`, then the optional arm64 Image, uImage, initrd and
-- | modules artifacts. Each step is a named `Aff` action; the host details
-- | live in `VmlinuxBuilder.Shell`.
module VmlinuxBuilder.Build (build) where

import Prelude

import Data.Either (Either(..))
import Data.Foldable (for_)
import Data.Maybe (Maybe(..), fromMaybe)
import Data.String (Pattern(..), contains)
import Data.String.Regex (test)
import Data.String.Regex.Flags (ignoreCase)
import Data.String.Regex.Unsafe (unsafeRegex)
import Effect.Aff (Aff, attempt, throwError)
import Effect.Exception (error)
import VmlinuxBuilder.DefaultConfig (defaultConfig)
import VmlinuxBuilder.Log as Log
import VmlinuxBuilder.Options (ConfigMode(..), Options, Source(..), UimageOptions, isBoardBuild, outputLabel)
import VmlinuxBuilder.Shell (capture, chdir, copyFile, cwd, exists, fetchText, machineArch, nproc, readText, rename, run, runQuiet, runWithYes, sudoPrefix, which, writeText)
import VmlinuxBuilder.Version (gitRef)
import VmlinuxBuilder.Zfs (KernelLine, configHasZfs, configWithModules, configWithZfs, developmentBranch, isCommit, kernelLine, parseZfsMeta, zfsConfigureArgs, zfsEnabled, zfsForKernel, zfsSupports)

linuxStable :: String
linuxStable = "linux-stable"

upstreamRepo :: String
upstreamRepo = "git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git"

isArm64 :: String -> Boolean
isArm64 arch = arch == "aarch64" || arch == "arm64"

-- | Build the kernel described by `opts`, leaving the artifacts in
-- | `linux-stable/` like the original.
build :: Options -> Aff Unit
build opts = do
  announce opts
  let withZfs = zfsEnabled opts.zfs { board: isBoardBuild opts }
  when withZfs $ Log.step $
    "OpenZFS " <> Log.cyan (fromMaybe "(from the compatibility matrix)" opts.zfsVersion)
      <> " will be built into the kernel (CONFIG_ZFS=y); pass --no-zfs to skip."

  installBuildDeps
  checkoutKernel opts.source
  configure opts.config
  -- From here on the working directory is the kernel tree.
  run [ "make", "prepare" ]
  n <- nproc
  when withZfs $ buildInZfs opts.zfsRepo opts.zfsVersion

  code <- runWithYes [ "make", "vmlinux", "-j" <> n ]
  when (code /= 0) $ throwError $ error ("make vmlinux exited with " <> show code)

  arch <- machineArch
  let vmlinux = "vmlinux-" <> outputLabel opts <> "." <> arch
  rename "vmlinux" vmlinux
  dir <- cwd
  Log.success "vmlinux built successfully!"
  Log.info ("You can find the vmlinux file in " <> Log.cyan (dir <> "/" <> vmlinux))

  when (isArm64 arch) $ emitImage n (outputLabel opts) arch
  for_ opts.uimage (uimage arch n)
  when opts.initrd $ initrd arch n
  when opts.modules $ modules n

announce :: Options -> Aff Unit
announce opts = case opts.source of
  Upstream v -> Log.info ("Building vmlinux for Linux kernel " <> Log.cyan (gitRef v))
  Custom c -> Log.info ("Building vmlinux from " <> Log.cyan c.repo <> " @ " <> Log.cyan c.ref)

-- | The kernel's own build dependencies, where apt is available. Failure is
-- | not fatal: an image may already have them, or lack the privilege.
installBuildDeps :: Aff Unit
installBuildDeps = aptInstall
  [ "git", "build-essential", "flex", "bison", "libncurses5-dev", "libssl-dev", "gcc", "bc", "libelf-dev", "pahole" ]

aptInstall :: Array String -> Aff Unit
aptInstall packages = do
  hasApt <- which "apt-get"
  when hasApt do
    sudo <- sudoPrefix
    _ <- attempt (run (sudo <> [ "apt-get", "install", "-y" ] <> packages))
    pure unit

-- | A shallow clone at the ref, or a shallow fetch into the tree kept from
-- | a previous build, reset hard to the ref.
checkoutKernel :: Source -> Aff Unit
checkoutKernel source = do
  let
    { url, ref } = case source of
      Upstream v -> { url: upstreamRepo, ref: gitRef v }
      Custom c -> { url: c.repo, ref: c.ref }
  present <- exists linuxStable
  if not present then
    run [ "git", "clone", "--depth=1", "--branch", ref, url, linuxStable ]
  else do
    shallow <- attempt (run [ "git", "-C", linuxStable, "fetch", "--depth=1", "origin", ref ])
    case shallow of
      Right _ -> pure unit
      Left _ -> run [ "git", "-C", linuxStable, "fetch", "origin", ref ]
    chdir linuxStable
    run [ "rm", "-rf", "Documentation/Kbuild" ]
    run [ "make", "mrproper" ]
    run [ "git", "checkout", "-f", ref ]
    chdir ".."

-- | Write the kernel tree's `.config` and change into the tree.
configure :: ConfigMode -> Aff Unit
configure = case _ of
  BoardDefconfig name -> do
    chdir linuxStable
    Log.info $ "Using board defconfig " <> Log.cyan name
      <> " as base (board essentials win); layering default config underneath."
    run [ "make", name ]
    copyFile ".config" "board.config"
    -- The default is the fragment and the board config comes last, so the
    -- board wins on every conflicting symbol.
    writeText "default.config" defaultConfig
    run [ "scripts/kconfig/merge_config.sh", "-m", "default.config", "board.config" ]
    -- Old BSP trees fail to build with modern GCC when warnings are fatal.
    merged <- readText ".config"
    writeText ".config" (merged <> "\n# CONFIG_WERROR is not set\n")
    run [ "make", "olddefconfig" ]
  MergeConfig src -> do
    chdir linuxStable
    existing <-
      if test (unsafeRegex "^https?://" ignoreCase) src then do
        Log.info ("Fetching existing config from " <> Log.cyan src)
        fetchText src
      else do
        Log.info ("Merging existing config " <> Log.cyan src <> " with default config (provided config overrides)")
        readText src
    -- Provided config last: kconfig keeps the last assignment it reads.
    writeText ".config" (defaultConfig <> "\n" <> existing <> "\n")
    run [ "make", "olddefconfig" ]
  DefaultConfig -> do
    present <- exists ".config"
    unless present do
      Log.warn "No .config file found in the current directory. Using default configuration."
      writeText ".config" defaultConfig
    chdir linuxStable
    copyFile "../.config" ".config"

-- | Put `ref` of the OpenZFS repository at `zfsDir`: a tag or branch is
-- | cloned shallowly, a commit is fetched by hash; an existing tree is
-- | re-fetched and reset, so it is reused across builds like `linux-stable`.
checkoutZfs :: String -> String -> String -> Aff Unit
checkoutZfs zfsDir repo ref = do
  present <- exists zfsDir
  fresh <-
    if present then pure false
    else if isCommit ref then do
      run [ "git", "init", "-q", zfsDir ]
      run [ "git", "-C", zfsDir, "remote", "add", "origin", repo ]
      pure false
    else do
      run [ "git", "clone", "--depth=1", "--branch", ref, repo, zfsDir ]
      pure true
  unless fresh do
    shallow <- attempt (run [ "git", "-C", zfsDir, "fetch", "--depth=1", "origin", ref ])
    case shallow of
      Right _ -> pure unit
      Left _ -> run [ "git", "-C", zfsDir, "fetch", "origin", ref ]
    run [ "git", "-C", zfsDir, "checkout", "-f", "FETCH_HEAD" ]
    -- A tree configured for another kernel keeps stale probes around.
    _ <- runQuiet [ "make", "-C", zfsDir, "distclean" ]
    pure unit

showLine :: Maybe KernelLine -> String
showLine = case _ of
  Just k -> show k.major <> "." <> show k.minor
  Nothing -> "?"

-- | Copy OpenZFS into the kernel tree (the working directory) and switch it
-- | on. OpenZFS's `copy-builtin` does the copying; what this guards against
-- | is documented beside each step.
buildInZfs :: String -> Maybe String -> Aff Unit
buildInZfs repo requested = do
  kernelDir <- cwd
  Log.step "Building OpenZFS into the kernel..."

  -- autogen needs the autotools; `--with-config=kernel` needs nothing else.
  aptInstall [ "autoconf", "automake", "libtool", "gawk" ]

  -- The kernel this tree is, from the tree itself: right for a `.y` branch
  -- and for a custom repository alike.
  kernel <- kernelLine <$> capture [ "make", "-s", "-C", kernelDir, "kernelversion" ]
  let zfsDir = kernelDir <> "/../zfs"

  -- Which OpenZFS: what was asked for, else the matrix's row for this kernel
  -- line, else — a kernel newer than every row — master, where support for
  -- a new kernel lands first. Whatever is checked out is verified against
  -- the kernel through its own META, the two bounds configure enforces, so
  -- a wrong row or request fails here with both numbers instead of inside
  -- configure after the clone.
  { version, probing } <- case requested, kernel of
    Just v, _ -> pure { version: v, probing: false }
    Nothing, Nothing -> throwError $ error
      "Could not read the kernel version from the tree (make kernelversion); pass --zfs-version <ref>."
    Nothing, Just k -> case zfsForKernel k of
      Just row -> do
        Log.step $ "Compatibility matrix: OpenZFS " <> row.version <> " (" <> row.ref
          <> (if row.development then ", development snapshot" else "") <> ") for Linux " <> showLine (Just k) <> "."
        pure { version: row.ref, probing: false }
      Nothing -> do
        Log.warn $ "No row of the compatibility matrix covers Linux " <> showLine (Just k)
          <> "; trying OpenZFS " <> developmentBranch <> ", where support for new kernels lands first."
        pure { version: developmentBranch, probing: true }
  checkoutZfs zfsDir repo version
  meta <- parseZfsMeta <$> readText (zfsDir <> "/META")
  for_ kernel \k -> unless (zfsSupports meta k) $ throwError $ error $
    "OpenZFS " <> version <> " (" <> fromMaybe "?" meta.version <> ") builds against Linux "
      <> showLine meta.linuxMinimum <> "–" <> showLine meta.linuxMaximum <> ", not " <> showLine (Just k) <> ". "
      <> case requested, probing of
        _, true -> "Nothing supports this kernel yet; pass --zfs-version <ref> to try one anyway, or --no-zfs."
        Just _, _ -> "Pass a ref whose META covers it, or drop --zfs-version to use the matrix."
        Nothing, _ -> "The matrix row is wrong for this tree; fix zfsCompatibility in VmlinuxBuilder.Zfs."
  when probing do
    -- Name the exact snapshot, so the kernel can be reproduced and a row added.
    snapshot <- capture [ "git", "-C", zfsDir, "rev-parse", "HEAD" ]
    Log.warn $ "Using OpenZFS " <> fromMaybe developmentBranch meta.version <> " development snapshot "
      <> snapshot <> "; add it to zfsCompatibility once it proves out."
  Log.step $ "OpenZFS " <> Log.cyan version <> " (" <> fromMaybe "?" meta.version <> ", Linux "
    <> showLine meta.linuxMinimum <> "–" <> showLine meta.linuxMaximum <> ") for kernel " <> showLine kernel

  -- The configure probes are external-module builds, which Kbuild refuses
  -- on a module-less config: modules on for the probe only.
  saved <- readText ".config"
  let probe = configWithModules saved
  when (probe /= saved) do
    writeText ".config" probe
    run [ "make", "olddefconfig" ]
    run [ "make", "modules_prepare" ]

  chdir zfsDir
  run [ "sh", "autogen.sh" ]
  run ([ "./configure" ] <> zfsConfigureArgs kernelDir)
  -- Pristine fs/Kconfig and fs/Makefile: copy-builtin appends to both
  -- unguarded, so a reused tree would accumulate hooks.
  run [ "git", "-C", kernelDir, "checkout", "-f", "--", "fs/Kconfig", "fs/Makefile" ]
  run [ "./copy-builtin", kernelDir ]
  chdir kernelDir

  -- copy-builtin hooks fs/Kconfig by inserting before the ext2 entry and
  -- says nothing if that anchor is gone.
  fsKconfig <- readText "fs/Kconfig"
  unless (contains (Pattern "source \"fs/zfs/Kconfig\"") fsKconfig) $
    writeText "fs/Kconfig" (fsKconfig <> "\nsource \"fs/zfs/Kconfig\"\n")

  -- Back to the real config, modules as they were, now with ZFS on.
  writeText ".config" (configWithZfs saved)
  run [ "make", "olddefconfig" ]
  after <- readText ".config"
  unless (configHasZfs after) $ throwError $ error
    "CONFIG_ZFS=y did not survive olddefconfig: a dependency (BLOCK, EFI_PARTITION) is off in this config."
  -- Kconfig changed, so the prepared headers are regenerated before the build.
  run [ "make", "prepare" ]
  Log.success "OpenZFS sources in tree, CONFIG_ZFS=y."

-- | The raw arm64 boot Image beside the ELF vmlinux, with its checksum.
emitImage :: String -> String -> String -> Aff Unit
emitImage n label arch = do
  run [ "make", "Image", "-j" <> n ]
  let image = "Image-" <> label <> "." <> arch
  copyFile "arch/arm64/boot/Image" image
  checksum image
  dir <- cwd
  Log.success ("boot Image built: " <> Log.cyan (dir <> "/" <> image))

checksum :: String -> Aff Unit
checksum file = do
  sum <- capture [ "sha256sum", file ]
  writeText (file <> ".sha256") (sum <> "\n")

-- | Wrap the arm64 Image as a U-Boot uImage with mkimage.
uimage :: String -> String -> UimageOptions -> Aff Unit
uimage arch n u
  | not (isArm64 arch) = Log.warn ("--uimage requested but arch is " <> arch <> " (not arm64); skipping uImage.")
  | otherwise = do
      Log.step "Generating uImage..."
      aptInstall [ "u-boot-tools" ]
      run [ "make", "Image", "-j" <> n ]
      source <-
        if u.comp == "gzip" then do
          run [ "gzip", "-9", "-k", "-f", "arch/arm64/boot/Image" ]
          pure "arch/arm64/boot/Image.gz"
        else pure "arch/arm64/boot/Image"
      krel <- capture [ "make", "-s", "kernelrelease" ]
      let name = fromMaybe ("Linux " <> krel) u.name
      run
        [ "mkimage", "-A", u.arch, "-O", u.os, "-T", u.type_, "-C", u.comp
        , "-a", u.load, "-e", u.entry, "-n", name, "-d", source, "uImage"
        ]
      checksum "uImage"
      dir <- cwd
      Log.success ("uImage built: " <> Log.cyan (dir <> "/uImage"))

-- | An initrd for this kernel release, and a uInitrd on arm64.
initrd :: String -> String -> Aff Unit
initrd arch n = do
  Log.step "Generating initrd..."
  aptInstall [ "initramfs-tools", "u-boot-tools" ]
  sudo <- sudoPrefix
  krel <- capture [ "make", "-s", "kernelrelease" ]
  dir <- cwd
  hasModules <- contains (Pattern "CONFIG_MODULES=y") <$> readText ".config"
  if hasModules then do
    -- `make vmlinux` never builds modules; mkinitramfs wants them installed.
    run [ "make", "modules", "-j" <> n ]
    run (sudo <> [ "make", "modules_install" ])
    run [ "make", "modules_install", "INSTALL_MOD_PATH=" <> dir <> "/modules-out" ]
  else
    run (sudo <> [ "mkdir", "-p", "/lib/modules/" <> krel ])
  let file = "initrd.img-" <> krel
  run (sudo <> [ "mkinitramfs", "-o", file, krel ])
  Log.success ("initrd built: " <> Log.cyan (dir <> "/" <> file))
  when (isArm64 arch) do
    run [ "mkimage", "-A", "arm64", "-O", "linux", "-T", "ramdisk", "-C", "gzip", "-n", "uInitrd " <> krel, "-d", file, "uInitrd" ]
    Log.success ("uInitrd built: " <> Log.cyan (dir <> "/uInitrd"))

-- | The loadable modules as `modules-<release>.tar.gz`; a no-op without
-- | CONFIG_MODULES.
modules :: String -> Aff Unit
modules n = do
  hasModules <- contains (Pattern "CONFIG_MODULES=y") <$> readText ".config"
  if not hasModules then
    Log.warn "--modules requested but CONFIG_MODULES is not enabled; skipping."
  else do
    Log.step "Archiving lib/modules..."
    krel <- capture [ "make", "-s", "kernelrelease" ]
    dir <- cwd
    run [ "make", "modules", "-j" <> n ]
    run [ "make", "modules_install", "INSTALL_MOD_PATH=" <> dir <> "/modules-out" ]
    -- The build/source symlinks point back into the tree; dereferencing
    -- them would archive the whole kernel, so they are left out.
    let tarball = "modules-" <> krel <> ".tar.gz"
    run [ "tar", "--exclude=build", "--exclude=source", "-czf", tarball, "-C", "modules-out", "lib/modules" ]
    checksum tarball
    Log.success ("modules archived: " <> Log.cyan (dir <> "/" <> tarball))
