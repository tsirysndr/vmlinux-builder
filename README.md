# vmlinux-builder

[![release](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml/badge.svg)](https://github.com/tsirysndr/vmlinux-builder/actions/workflows/ci.yml)

📦 **vmlinux-builder** is a lightweight Bash-based tool to fetch, configure, and build the Linux kernel `vmlinux` image for a given version — ideal for use with Firecracker microVMs or other kernel-related debugging/testing tasks.

## ✨ Features

- Builds any stable Linux kernel version (e.g., `6.1`, `6.1.12`, `6.1.y`)
- Uses a custom `.config` for reproducible builds
- Outputs a ready-to-use `vmlinux-X.Y` file
- Easily integrated into CI pipelines (e.g., GitHub Actions)

## 🛠 Prerequisites

Ensure you have the following dependencies installed:

```bash
sudo apt-get install -y git build-essential flex bison libncurses5-dev \
libssl-dev gcc bc libelf-dev pahole
```

## 🚀 Usage
Clone the repo and provide a kernel version:

```bash
./build.sh 6.16.y
```

**Note:** Ensure a valid .config file is present in the root directory before running.


### Example output

```
vmlinux built successfully!
You can find the vmlinux file in linux-stable/vmlinux-6.16
```

## 📦 GitHub Actions

This repo includes a GitHub Actions workflow (`.github/workflows/ci.yml`) that:

- Triggers on tag push (e.g. git tag 6.16.y && git push origin 6.16.y)
- Builds the vmlinux for that version
- Publishes the resulting vmlinux-X.Y as a GitHub Release asset

## 📄 License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.