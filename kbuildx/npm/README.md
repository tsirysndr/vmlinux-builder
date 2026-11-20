# @tsiry/kbuildx

Install the [`kbuildx`](https://github.com/tsirysndr/vmlinux-builder/tree/main/kbuildx)
binary via npm.

`kbuildx` is an interactive **Linux and BSD kernel builder**: a Ratatui terminal
interface plus a scriptable CLI that manages source checkouts, applies kernel
configurations and produces kernel, boot, module and rootfs artifacts. It builds
inside a [bsdkrun](https://github.com/tsirysndr/bsdkrun) microVM, or directly on a
Linux host with `apk`, `apt-get` or `dnf`.

## Install

```sh
npm install -g @tsiry/kbuildx
# then
kbuildx --help
```

Or run it without a global install:

```sh
npx @tsiry/kbuildx list
```

On install, a prebuilt `kbuildx` for your platform is downloaded from the
matching [GitHub release](https://github.com/tsirysndr/vmlinux-builder/releases)
(tag `kbuildx-v<version>`) and its SHA-256 verified.

## Supported platforms

| OS    | Arch    | Notes                         |
| ----- | ------- | ----------------------------- |
| macOS | `arm64` | Apple Silicon only (no Intel) |
| Linux | `x64`   |                               |
| Linux | `arm64` |                               |

Any other platform/arch (Windows, Intel macOS, 32-bit, …) is **unsupported** and
the install fails with a clear message.

## Runtime prerequisites

Unless you build directly on a Linux host (`--host`), kbuildx drives bsdkrun for
its build sandbox:

```sh
npm install -g @bsdkrun/cli
bsdkrun probe
```

Linux needs access to `/dev/kvm`; macOS uses Hypervisor.framework.

## Environment variables

| Variable                | Effect                                              |
| ----------------------- | --------------------------------------------------- |
| `KBUILDX_SKIP_DOWNLOAD` | Skip the download entirely (package development).   |
| `KBUILDX_BINARY`        | Copy this local binary instead of downloading.      |
| `KBUILDX_DOWNLOAD_BASE` | Fetch the release archive from this base URL.       |

## License

MIT
