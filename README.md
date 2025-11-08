# vmlinux-builder

[![ci](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/tests.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/tests.yml)
[![release](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml)

📦 **vmlinux-builder** is a lightweight TypeScript/Deno-based tool to fetch, configure, and build the Linux kernel `vmlinux` image for a given version — ideal for use with Firecracker microVMs or other kernel-related debugging/testing tasks.

## ✨ Features

- Builds any stable Linux kernel version (e.g., `6.1`, `6.1.12`, `6.1.y`)
- Supports multiple architectures (x86_64, aarch64)
- Enhanced kernel configuration handling with customizable options
- Uses a custom `.config` for reproducible builds
- Outputs a ready-to-use `vmlinux-X.Y-[arch]` file
- Written in TypeScript with Deno for better type safety and cross-platform compatibility
- Colored output using Chalk for improved readability
- Easily integrated into CI pipelines (e.g., GitHub Actions)

## 🛠 Prerequisites

### System Dependencies

Ensure you have the following dependencies installed:

```bash
sudo apt-get install -y git build-essential flex bison libncurses5-dev \
libssl-dev gcc bc libelf-dev pahole
```

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

### Example output

```sh
Building vmlinux for Linux kernel 6.16 (x86_64)
vmlinux built successfully!
You can find the vmlinux file in /path/to/linux-stable/vmlinux-6.16.x86_64
```

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
