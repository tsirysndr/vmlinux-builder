# vmlinux-builder

[![ci](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/tests.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/tests.yml)
![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/downloads/tsirysndr/vmlinux-builder/total)
[![Docker Pulls](https://img.shields.io/docker/pulls/tsiry/vmlinux-builder)](https://hub.docker.com/r/tsiry/vmlinux-builder)
[![JSR](https://jsr.io/badges/@tsiry/vmlinux-builder)](https://jsr.io/@tsiry/vmlinux-builder)
[![release](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml)

📦 **vmlinux-builder** is a lightweight TypeScript/Deno-based tool to fetch, configure, and build the Linux kernel `vmlinux` image for a given version — ideal for use with Firecracker microVMs or other kernel-related debugging/testing tasks.

## ✨ Features

- Builds any stable Linux kernel version (e.g., `6.1`, `6.1.12`, `6.1.y`)
- Enhanced kernel configuration handling with customizable options
- Uses a custom `.config` for reproducible builds
- Outputs a ready-to-use `vmlinux-X.Y-[arch]` file
- Builds **OpenZFS into the kernel** (`CONFIG_ZFS=y`, pinned to one OpenZFS release) so a guest can import and create pools with nothing but `zpool` in its image
- Written in TypeScript with Deno for better type safety and cross-platform compatibility
- Colored output using Chalk for improved readability
- Easily integrated into CI pipelines (e.g., GitHub Actions)

## 🛠 Prerequisites

### System Dependencies

Ensure you have the following dependencies installed:

```bash
sudo apt-get install -y git build-essential flex bison libncurses5-dev \
libssl-dev gcc bc libelf-dev pahole autoconf automake libtool gawk
```

The last four are for OpenZFS's `autogen.sh`; skip them if you build with `--no-zfs`.

### Deno Runtime

Install Deno if you haven't already:

```bash
curl -fsSL https://deno.land/install.sh | sh
```

Or follow the [official Deno installation guide](https://docs.deno.com/runtime/getting_started/installation).

## 🚀 Usage

```bash
deno run -A jsr:@tsiry/vmlinux-builder 6.17.7
```

Or with docker:

```bash
docker volume create linux-build
docker run --rm -it -v linux-build:/app tsiry/vmlinux-builder:latest 6.17.7
```

### Supported Version Formats

- `6.1` - Major.Minor version
- `6.1.12` - Specific patch version
- `6.1.y` - Latest from maintenance branch
- `v6.1.12` - Version with 'v' prefix (automatically normalized)

### Kernel Configuration

The tool provides enhanced kernel configuration handling with:

- Support for TUN and TUN_VNET_CROSS_LE
- Various netfilter options enabled
- Customizable configuration through environment variables
- Reproducible builds with consistent config options

### OpenZFS

Every upstream build carries OpenZFS in the kernel. Before `make vmlinux`, the
tool clones the OpenZFS pinned for that kernel line, configures it kernel-only
(`--with-config=kernel --enable-linux-builtin`), runs its `copy-builtin` into
the tree and sets `CONFIG_ZFS=y`.

Which OpenZFS is decided by a **compatibility matrix kept in the code**
(`ZFS_COMPATIBILITY` in `zfs.ts`, `zfsCompatibility` in the PureScript port),
each row being an OpenZFS ref and the kernel lines its own `META` declares.
OpenZFS's configure refuses a kernel outside those bounds, which is what the
first 7.3 build ran into. The kernel line is read from the checked-out tree
(`make kernelversion`), so it is right for `.y` branches and custom trees too:

| Kernel line     | OpenZFS                                                                        |
| --------------- | ------------------------------------------------------------------------------ |
| 4.18 – 7.2      | `zfs-2.4.4`, the newest release covering it                                    |
| 7.3             | master commit `d1a5b59` (2.4.99): support is on master only, the backport to 2.4.5 is still open |
| beyond the rows | `master` is tried and verified through its `META`; the build stops if even that cannot build against the kernel |

Whatever is checked out is verified against the kernel through its `META`
before configure runs, so a wrong row, or a `--zfs-version` that does not
cover the kernel, fails at once with both numbers. Add a row when a release
ships with a new `Linux-Maximum`. The result is a
kernel where `zpool`/`zfs` from any distribution package work without a module:
nothing to load, nothing to match against a kernel version.

- `--no-zfs` — leave it out.
- `--zfs-version <ref>` — another OpenZFS tag, branch or commit, overriding the table.
- `--zfs-repo <url>` — another OpenZFS repository.
- Board builds (`--repo`, `--defconfig`) are left alone by default; pass `--zfs`
  to opt in.

`CONFIG_ZFS` depends on `BLOCK` and `EFI_PARTITION`; the build fails rather than
silently dropping the symbol if a merged config has turned those off.

### Example output

```sh
Building vmlinux for Linux kernel 6.16 (x86_64)
vmlinux built successfully!
You can find the vmlinux file in /path/to/linux-stable/vmlinux-6.16.x86_64
```

## 🟣 PureScript port

[`purescript/`](purescript/) is a port of `build.ts` and `config.ts` with the
same flags and the same bundled config (imported from `default-config.ts`, not
copied). It is what the workflows run:

```sh
cd purescript && spago build
node bin/vmlinux-builder.js 6.17.7 --no-zfs
```

See [`purescript/README.md`](purescript/README.md) for the config library.

## 📦 GitHub Actions

This repo includes a GitHub Actions workflow (`.github/workflows/ci.yml`) that:

- Triggers on tag push (e.g. `git tag 6.16.y && git push origin 6.16.y`)
- Builds the vmlinux for both x86_64 and aarch64 architectures
- Publishes the resulting `vmlinux-X.Y-[arch]` files as GitHub Release assets
- Includes SHA256 checksums for all released files

## 🧩 Config API Usage

The `config.ts` module provides a type-safe API for parsing, validating, extracting, and serializing Linux kernel `.config` files.

### Parse a kernel config file

```ts
import { KernelConfigParser } from '@tsiry/vmlinux-builder';
const content = await Deno.readTextFile('path/to/.config');
const config = KernelConfigParser.parse(content);
```

### Extract config categories

```ts
import { KernelConfigParser } from '@tsiry/vmlinux-builder';
const processor = KernelConfigParser.extractProcessorConfig(config);
const security = KernelConfigParser.extractSecurityConfig(config);
const networking = KernelConfigParser.extractNetworkingConfig(config);
const filesystem = KernelConfigParser.extractFilesystemConfig(config);
```

### Serialize config to different formats

```ts
import { KernelConfigSerializer } from '@tsiry/vmlinux-builder';
const asConfig = KernelConfigSerializer.toConfig(config); // .config format
const asJSON = KernelConfigSerializer.toJSON(config);
const asTOML = KernelConfigSerializer.toTOML(config);
const asYAML = KernelConfigSerializer.toYAML(config);
```

### Validate a config

```ts
import { validateKernelConfig } from '@tsiry/vmlinux-builder';
const result = validateKernelConfig(config);
if (!result.success) {
  throw new Error('Invalid kernel config');
}
```

See `config.ts` for more advanced usage and options.

The script is written in TypeScript and runs on Deno. Key features:

- **Type-safe**: Full TypeScript support with type checking
- **Modern**: Uses Deno's native APIs for file operations and process management
- **Colored output**: Enhanced user experience with Chalk

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
