# kbuildx

`kbuildx` is an interactive Linux and BSD kernel builder. It combines a Ratatui terminal interface with a scriptable command-line API, manages source checkouts, applies kernel configurations, and produces kernel, boot, module, and rootfs artifacts.

On Linux, builds can run directly on hosts with `apk`, `apt-get`, or `dnf`. Alpine is detected automatically. Otherwise, `kbuildx` creates and reuses an Alpine microVM through [bsdkrun](https://github.com/tsirysndr/bsdkrun). macOS always uses bsdkrun.

## Highlights

- Ratatui home screen with live, auto-scrolling build logs.
- Fuzzy kernel-version picker opened with `/`.
- Fuzzy kernel-config picker opened with `c`.
- Start and stop builds without leaving the interface.
- Stable tag, maintenance branch, and custom repository support.
- Built-in kernel configuration with CLI and TUI overrides.
- Optional board defconfig and external config merging.
- Optional modules, initrd, arm64 Image, uImage, and uInitrd output.
- Persistent kernel checkout reused across builds.
- Native Linux builds with `apk`, `apt-get`, or `dnf`, plus a bsdkrun fallback.
- Native FreeBSD and NetBSD source builds inside persistent bsdkrun machines.
- Optional bootable BSD rootfs bundles with bsdkrun-agent injected.

## Installation

### Build from source

Requirements:

- Rust toolchain compatible with edition 2024.
- Git.
- On hosts not using Linux `--host` mode, the `bsdkrun` CLI and a supported virtualization backend.

```sh
cargo build --release --locked
install -m 0755 target/release/kbuildx /usr/local/bin/kbuildx
```

Install bsdkrun when direct Linux host mode is not being used:

```sh
npm install -g @bsdkrun/cli
bsdkrun probe
```

Linux requires access to `/dev/kvm`. macOS uses Hypervisor.framework.

### Docker image

The included multi-stage Dockerfile builds a self-contained Alpine image:

```sh
docker build -t kbuildx .
docker run --rm -it -v "$PWD:/workspace" -w /workspace kbuildx
```

Because the image provides `apk`, `kbuildx` builds directly inside the container and does not start a nested sandbox.

## Interactive TUI

Launch the TUI by running `kbuildx` without a subcommand:

```sh
kbuildx
```

The explicit form is also available:

```sh
kbuildx tui
```

The home screen shows the selected kernel version, number of config overrides, current build state, live logs, and a persistent shortcut bar.

### Keyboard shortcuts

| Key                   | Action                                           |
| --------------------- | ------------------------------------------------ |
| `b`                   | Start building the selected kernel version       |
| `x`                   | Stop the running build                           |
| `/`                   | Open the fuzzy kernel-version picker             |
| `c`                   | Open the fuzzy kernel-config picker              |
| `r`                   | Edit sandbox CPU and memory options              |
| `o`                   | Override all arguments passed to `kbuildx build` |
| `l`                   | Toggle the fullscreen live-log view              |
| `Up` / `Down`         | Scroll logs and pause automatic following        |
| `PageUp` / `PageDown` | Scroll logs by ten lines                         |
| `End`                 | Resume following live output                     |
| `?`                   | Open or close the help overlay                   |
| `q`                   | Quit and stop an active build                    |
| `Ctrl-C`              | Stop an active build and quit                    |

### Kernel-version picker

Press `/`, type any part of a version, move with the arrow keys, and press `Enter`. Matching uses fuzzy scoring, and final releases are ordered newest first. Release candidates are excluded.

Press `Esc` to cancel without changing the selected version.

### Kernel-config picker

Press `c` and type a config symbol such as `BPF`, `KVM`, or `NETFILTER`. The picker searches boolean and tristate options from the built-in config.

Pressing `Enter` cycles the selected value:

```text
n → y → m → n
```

The selected value is stored as a build override and passed through the normal CLI build pipeline. Kernel Kconfig normalization still has the final say when a symbol is unavailable or has unmet dependencies.

### Resource editor

Press `r` to edit the vCPU and memory values used by TUI builds. Use `Tab`, `Up`, or `Down` to switch fields, enter numeric values, and press `Enter` to apply. Both values must be greater than zero. Press `Esc` to cancel.

The selected values are passed to the build command as `--cpus` and `--memory`. The CPU value defaults to the number of logical CPUs available on the host. These values configure a bsdkrun sandbox; direct host builds naturally use the host's available resources.

### Build-options editor

Press `o` to edit the complete argument list passed after `kbuildx build`. This allows every build option—including custom repositories, branches, defconfigs, config merges, host mode, modules, initrd, and uImage settings—to be overridden from the TUI. Shell-style single quotes, double quotes, and backslash escapes are supported.

The editor starts with the currently selected kernel version, CPU count, and memory limit. Press `Enter` to apply the complete argument list or `Esc` to cancel. Once applied, this list replaces the TUI-generated version and resource arguments; config options selected with `c` are still appended as repeatable `--set-config` arguments.

### Live build logs

Build stdout and stderr share a real pseudo-terminal and are read as raw chunks, so nested tools see the same terminal environment as normal CLI mode. Output is rendered as it arrives, including carriage-return progress updates that do not end with a newline. The log view follows the newest output automatically. Scrolling upward pauses following; press `End` to return to real-time output.

Press `l` to expand the logs across the terminal and press `l` again to return to the home screen. Log scrolling, auto-follow, and build controls remain available in fullscreen mode.

The in-memory log is capped at 10,000 lines to keep long builds responsive.

## Command-line usage

### List kernel versions

```sh
kbuildx ls
kbuildx ls --refresh
```

The version list is cached in `/tmp/kernel_versions.txt` for 24 hours. `--refresh` fetches tags immediately.

### Build a stable release

```sh
kbuildx build 7.1.8
```

Versions may include the leading `v`:

```sh
kbuildx build v7.1.8
```

### Build a stable maintenance branch

```sh
kbuildx build 6.6.y
```

This resolves to the upstream `linux-6.6.y` branch and uses `6.6` in artifact names.

### Build FreeBSD or NetBSD

BSD builds always run inside the matching persistent bsdkrun machine:

```sh
kbuildx build 15.1 --os freebsd
kbuildx build 10.1 --os netbsd
kbuildx build current --os netbsd
```

FreeBSD release numbers resolve to `releng/<version>` in the official source repository. NetBSD releases resolve to `netbsd-<version>-RELEASE`, while `current` resolves to `trunk`. Override either source with the existing `--repo`, `--branch`, and `--version` options.

The default kernel configuration is selected for the guest architecture. Pass `--defconfig` to select another FreeBSD kernel configuration or NetBSD kernel configuration:

```sh
kbuildx build 15.1 --os freebsd --defconfig GENERIC
kbuildx build current --os netbsd --defconfig MICROVM
```

### Build a complete BSD bootable bundle

Add `--bundle` to build a compressed bootable root filesystem alongside the kernel:

```sh
kbuildx build 15.1 --os freebsd --bundle --cpus 8 --memory 12288
kbuildx build current --os netbsd --bundle --cpus 8 --memory 12288
```

The bundle process follows bsdkrun's image workflows:

1. Build the matching kernel from source inside FreeBSD or NetBSD.
2. Assemble a native UFS or FFS root filesystem.
3. Copy the running image's `bsdkrun-agent` into the rootfs.
4. Install and enable the native rc.d startup service.
5. Export the raw image to the host.
6. Compress the image on the host exactly like bsdkrun's image workflows: `xz -T0 -6` for FreeBSD arm64, or `gzip -9` for FreeBSD amd64 and NetBSD, then generate checksums on the host.

BSD build machines use persistent 40 GiB volumes so source, packages, and build state survive subsequent invocations. `--host` and Linux-specific config/module/initrd/uImage flags are rejected for BSD targets.

### Select sandbox resources

```sh
kbuildx build 7.1.8 --cpus 8 --memory 8192
```

The CPU default is the host's available logical CPU count; memory defaults to 2048 MiB. `--mem` is an alias for `--memory`. Resource flags apply only when a bsdkrun sandbox is needed; direct host builds use the host resources.

### Build directly on a Linux host

```sh
kbuildx build 7.1.8 --host
```

Host mode is supported only on Linux. It detects the available package manager and installs the corresponding kernel build dependencies:

| Distribution family | Detected command | Initrd tool      |
| ------------------- | ---------------- | ---------------- |
| Alpine              | `apk`            | `mkinitfs`       |
| Debian / Ubuntu     | `apt-get`        | `mkinitramfs`    |
| Fedora / RHEL       | `dnf`            | `dracut`         |

Dependency installation runs directly when already root and uses `sudo` otherwise. If no supported package manager exists, host mode exits with an actionable error.

Alpine retains automatic host execution for compatibility with container usage. On Debian-, Ubuntu-, Fedora-, and RHEL-family systems, pass `--host` explicitly. macOS never permits host mode and always uses bsdkrun.

### Custom repository and branch

```sh
kbuildx build \
  --repo https://github.com/example/linux-board \
  --branch board-6.6 \
  --version board-6.6
```

`--branch` is required for custom repositories. `--ref` is an alias. The optional `--version` label controls artifact filenames.

### Override config symbols

`--set-config` is repeatable:

```sh
kbuildx build 7.1.8 \
  --set-config BPF=y \
  --set-config DEBUG_INFO=n \
  --set-config CONFIG_KVM=m
```

Both `NAME=value` and `CONFIG_NAME=value` forms are accepted. Supported override values are `y`, `m`, and `n`.

### Merge an external config

Merge a file located at the kernel checkout root:

```sh
kbuildx build 7.1.8 --merge-config board.config
```

`--config` is an alias. HTTP and HTTPS sources are also accepted:

```sh
kbuildx build 7.1.8 \
  --merge-config https://example.com/kernel.config
```

The built-in config is written first and the external config is appended afterward, so external assignments take precedence before `make olddefconfig` normalizes the result.

### Board defconfig

```sh
kbuildx build \
  --repo https://github.com/example/linux-board \
  --branch vendor-6.6 \
  --defconfig board_defconfig
```

The board defconfig is generated first. The built-in config is then layered underneath it with the board configuration last, so board-specific requirements win. `CONFIG_WERROR` is disabled for compatibility with older BSP trees.

### Modules

```sh
kbuildx build 7.1.8 --modules
```

When `CONFIG_MODULES=y`, modules are built, staged under `modules-out`, and archived as:

```text
modules-<kernelrelease>.tar.gz
modules-<kernelrelease>.tar.gz.sha256
```

The option is skipped with a warning when modules are disabled in the normalized config.

### Initrd

```sh
kbuildx build 7.1.8 --initrd
```

The distribution's initrd tool (`mkinitfs`, `mkinitramfs`, or `dracut`) generates `initrd.img-<kernelrelease>`. On arm64, an additional U-Boot `uInitrd` is generated. SHA-256 files are created alongside the artifacts.

### U-Boot uImage

```sh
kbuildx build 7.1.8 \
  --uimage \
  --uimage-arch arm \
  --uimage-os linux \
  --uimage-type kernel \
  --uimage-comp gzip \
  --uimage-load 0x41000000 \
  --uimage-entry 0x41000000 \
  --uimage-name "Linux board image"
```

uImage generation is arm64-only. The defaults match a conventional Linux kernel uImage and use no compression unless changed.

## Build execution model

### Direct Linux host

With `--host`, `kbuildx` detects `apk`, `apt-get`, or `dnf`, installs the platform-specific build dependencies, and runs checkout, configuration, and compilation directly. Alpine hosts are detected automatically even when `--host` is omitted.

The checkout is stored at:

```text
./linux
```

This mode is used automatically in Alpine containers and native Alpine systems. Other Linux distributions require `--host`.

### bsdkrun sandbox

When direct host mode is not selected, `kbuildx` creates or reuses an Alpine sandbox named:

```text
kbuildx_sandbox
```

The sandbox persists its `/linux` checkout between invocations. Requested CPU and memory values are applied before it starts. Existing sandboxes are restarted when necessary for resource changes.

The bsdkrun executable must be discoverable through `PATH` or `BSDKRUN_BIN`.

### BSD build machines

FreeBSD and NetBSD use separate persistent bsdkrun machines named from the OS and requested version. kbuildx boots the requested BSD image, installs native source-build dependencies with `pkg` or `pkgin`, updates the source checkout, and invokes the operating system's own build tools.

Prepared bsdkrun BSD images already contain the guest agent required by `bsdkrun exec`. Complete bundles copy that agent from the running build machine into the assembled rootfs and enable it at boot.

## Checkout behavior

For an existing valid checkout, `kbuildx`:

1. Fetches only the requested ref with shallow history.
2. Force-checks out `FETCH_HEAD` in detached mode.
3. Prints the selected tag/ref and short commit hash.

If the `linux` directory exists but is not a valid Git worktree, it is treated as an interrupted clone, removed, and cloned again.

Git checkout uses one worker for compatibility with VM-backed filesystems. The exact managed checkout path is registered as a Git safe directory when required.

## Configuration precedence

From lowest to highest effective priority:

1. Built-in `KernelConfig` defaults.
2. TUI or `--set-config` overrides.
3. External `--merge-config` content, when supplied.
4. Board defconfig, when `--defconfig` is supplied.
5. Kconfig dependency resolution performed by `make olddefconfig`.

Board-defconfig and merge-config modes are mutually exclusive in the build pipeline; when both are supplied, board-defconfig mode is used.

## Artifacts

The primary output is:

```text
vmlinux-<version>.<architecture>
vmlinux-<version>.<architecture>.sha256
```

On arm64, the raw boot image is also generated:

```text
Image-<version>.<architecture>
Image-<version>.<architecture>.sha256
```

Optional outputs include:

- `uImage` and `uImage.sha256`
- `initrd.img-<kernelrelease>` and checksum
- `uInitrd` and checksum on arm64
- `modules-<kernelrelease>.tar.gz` and checksum

Artifacts live in `./linux` during direct host builds. Sandbox builds keep their working files in `/linux` and copy the finished `vmlinux-<version>.<architecture>` plus its SHA-256 file into the host's `./linux` directory before returning successfully.

BSD kernel-only builds export:

```text
./freebsd/freebsd-<version>.<architecture>.kernel
./freebsd/freebsd-<version>.<architecture>.kernel.sha256
./netbsd/netbsd-<version>.<architecture>.kernel
./netbsd/netbsd-<version>.<architecture>.kernel.sha256
```

With `--bundle`, the matching directory also receives:

```text
<os>-<version>.<architecture>.img.gz
<os>-<version>.<architecture>.img.gz.sha256
freebsd-<version>.aarch64.img.xz
freebsd-<version>.aarch64.img.xz.sha256
```

The `.xz` variant is used only for FreeBSD arm64; other BSD bundles use `.gz`.

## GitHub Actions E2E tests

The sandbox E2E workflow:

- Runs on `ubuntu-latest` with KVM enabled.
- Installs `@bsdkrun/cli`.
- Verifies the bsdkrun runtime.
- Caches Rust dependencies and build output.
- Runs unit tests and checks the expanded CLI options.
- Exercises `ls --refresh`.
- Performs a full Linux 7.1.8 build using all runner CPUs and reported memory.
- Raises the host file-descriptor hard limit for Linux virtio-fs, which pins a descriptor per observed inode.

A separate host E2E workflow runs directly on `ubuntu-latest`:

- Does not install or invoke bsdkrun.
- Builds and tests the Rust CLI.
- Verifies that `--host` is exposed.
- Performs a full Linux 7.1.8 build with Ubuntu's `apt-get` toolchain.

## Troubleshooting

### `could not find the "bsdkrun" binary`

Install the CLI and verify its location:

```sh
npm install -g @bsdkrun/cli
command -v bsdkrun
bsdkrun probe
```

If needed, set the explicit path:

```sh
export BSDKRUN_BIN="$(command -v bsdkrun)"
```

### `/dev/kvm` is unavailable

The Linux sandbox requires KVM and permission to access `/dev/kvm`. Check:

```sh
test -r /dev/kvm -a -w /dev/kvm
bsdkrun probe
```

### `No file descriptors available`

Linux virtio-fs may pin one host descriptor per inode. Large kernel trees can exceed a 65,536 descriptor hard limit. Raise both limits before starting `kbuildx`, or run it through `prlimit` with sufficient privilege.

### `fatal: detected dubious ownership`

`kbuildx` automatically registers only its exact managed checkout path as safe. Avoid setting `safe.directory=*` globally.

### Build configuration warnings

Kernel versions may rename symbols or change their types. `make olddefconfig` reports stale assignments and normalizes the final `.config`. A warning is not fatal unless followed by a `make` error.

### Terminal display is not restored

Normal exits, `q`, and `Ctrl-C` restore the terminal. If the process is forcibly killed, run:

```sh
reset
```

## Development

```sh
cargo fmt -- --check
cargo test --locked
cargo check --locked
```

The TUI is implemented as a state machine in `src/tui.rs`. Builds are launched as child `kbuildx build` processes, keeping interactive and non-interactive behavior aligned.

## License

Use the license declared by the parent repository.
