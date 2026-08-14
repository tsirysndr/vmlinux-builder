# kbuildx

`kbuildx` is an interactive Linux kernel builder. It combines a Ratatui terminal interface with a scriptable command-line API, manages kernel source checkouts, applies a built-in kernel configuration, and produces boot and module artifacts.

On Alpine Linux, builds run directly on the host. On other systems, `kbuildx` creates and reuses an Alpine microVM through [bsdkrun](https://github.com/tsirysndr/bsdkrun).

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
- Native Alpine builds with an automatic bsdkrun fallback elsewhere.

## Installation

### Build from source

Requirements:

- Rust toolchain compatible with edition 2024.
- Git.
- On non-Alpine hosts, the `bsdkrun` CLI and a supported virtualization backend.

```sh
cargo build --release --locked
install -m 0755 target/release/kbuildx /usr/local/bin/kbuildx
```

Install bsdkrun when the host does not provide `apk`:

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

### Live build logs

Build stdout and stderr are read concurrently and rendered as they arrive. The log view follows the newest output automatically. Scrolling upward pauses following; press `End` to return to real-time output.

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

### Select sandbox resources

```sh
kbuildx build 7.1.8 --cpus 8 --memory 8192
```

Defaults are 2 vCPUs and 2048 MiB. `--mem` is an alias for `--memory`. Resource flags apply only when a bsdkrun sandbox is needed; direct Alpine-host builds use the host resources.

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

Alpine's `mkinitfs` generates `initrd.img-<kernelrelease>`. On arm64, an additional U-Boot `uInitrd` is generated. SHA-256 files are created alongside the artifacts.

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

### Alpine host

At startup, `kbuildx` executes `apk --version`. If it succeeds, dependency installation, checkout, configuration, and compilation run directly on the host.

The checkout is stored at:

```text
./linux
```

This mode is used automatically in Alpine containers and native Alpine systems.

### bsdkrun sandbox

When `apk` is unavailable, `kbuildx` creates or reuses an Alpine sandbox named:

```text
kbuildx_sandbox
```

The sandbox persists its `/linux` checkout between invocations. Requested CPU and memory values are applied before it starts. Existing sandboxes are restarted when necessary for resource changes.

The bsdkrun executable must be discoverable through `PATH` or `BSDKRUN_BIN`.

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

Artifacts live in `./linux` during direct Alpine builds and `/linux` inside the persistent bsdkrun sandbox otherwise.

## GitHub Actions E2E tests

The repository workflow:

- Runs on `ubuntu-latest` with KVM enabled.
- Installs `@bsdkrun/cli`.
- Verifies the bsdkrun runtime.
- Caches Rust dependencies and build output.
- Runs unit tests and checks the expanded CLI options.
- Exercises `ls --refresh`.
- Performs a full Linux 7.1.8 build using all runner CPUs and reported memory.
- Raises the host file-descriptor hard limit for Linux virtio-fs, which pins a descriptor per observed inode.

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
