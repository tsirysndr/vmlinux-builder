# vmlinux-builder

[![release](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml)

📦 **vmlinux-builder** is a lightweight TypeScript/Deno-based tool to fetch, configure, and build the Linux kernel `vmlinux` image for a given version — ideal for use with Firecracker microVMs or other kernel-related debugging/testing tasks.

## ✨ Features

- Builds any stable Linux kernel version (e.g., `6.1`, `6.1.12`, `6.1.y`)
- Uses a custom `.config` for reproducible builds
- Outputs a ready-to-use `vmlinux-X.Y` file
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

### Example output
```
Building vmlinux for Linux kernel 6.16
vmlinux built successfully!
You can find the vmlinux file in /path/to/linux-stable/vmlinux-6.16.x86_64
```

## 📦 GitHub Actions

This repo includes a GitHub Actions workflow (`.github/workflows/ci.yml`) that:

- Triggers on tag push (e.g. `git tag 6.16.y && git push origin 6.16.y`)
- Builds the vmlinux for that version
- Publishes the resulting `vmlinux-X.Y` as a GitHub Release asset

## 🔧 Development

The script is written in TypeScript and runs on Deno. Key features:

- **Type-safe**: Full TypeScript support with type checking
- **Cross-platform**: Works on Linux, macOS, and Windows (WSL)
- **Modern**: Uses Deno's native APIs for file operations and process management
- **Colored output**: Enhanced user experience with Chalk

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
