#!/bin/sh
# Install kbuildx from its GitHub releases:
#
#   curl -fsSL https://raw.githubusercontent.com/tsirysndr/vmlinux-builder/main/kbuildx/install.sh | sh
#
# Environment variables:
#
#   KBUILDX_VERSION=0.1.0       pin a release (default: the newest kbuildx-v*
#                               release). `v0.1.0` and `kbuildx-v0.1.0` are
#                               accepted too, as is a first argument:
#                               `sh -s -- 0.1.0`.
#   KBUILDX_INSTALL=~/.kbuildx  install root; the binary lands in its bin/
#
# The archive is verified against the .sha256 sidecar every release publishes.
#
# kbuildx releases share the vmlinux-builder repository with the kernel
# releases (tags like `7.3.5`), so "latest" is resolved through the GitHub
# API over the `kbuildx-v*` tags rather than /releases/latest, which names
# whichever release was published last.

set -eu

REPO="tsirysndr/vmlinux-builder"

say() { printf '%s\n' "$*"; }
err() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

main() {
    version="${1:-${KBUILDX_VERSION:-}}"
    root="${KBUILDX_INSTALL:-$HOME/.kbuildx}"
    bin_dir="$root/bin"

    command -v curl >/dev/null 2>&1 || err "curl is required"
    command -v tar >/dev/null 2>&1 || err "tar is required"

    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os $arch" in
    "Darwin arm64") triple="aarch64-apple-darwin" ;;
    "Linux x86_64") triple="x86_64-unknown-linux-gnu" ;;
    "Linux aarch64" | "Linux arm64") triple="aarch64-unknown-linux-gnu" ;;
    *)
        err "unsupported platform: $os/$arch
kbuildx ships prebuilt binaries for macOS/arm64 (Apple Silicon), Linux/x86_64
and Linux/aarch64. Elsewhere, build from source: cargo build --release --locked"
        ;;
    esac

    if [ -n "$version" ]; then
        # Tags are `kbuildx-vX.Y.Z`; accept the bare number and `vX.Y.Z` too.
        case "$version" in
        kbuildx-v*) tag="$version" ;;
        v*) tag="kbuildx-$version" ;;
        *) tag="kbuildx-v$version" ;;
        esac
    else
        tag="$(latest_tag)" || err "could not find a kbuildx release at https://github.com/$REPO/releases
Pin one with KBUILDX_VERSION=0.1.0 if the GitHub API is unreachable."
    fi
    asset="$tag-$triple.tar.gz"
    base="https://github.com/$REPO/releases/download/$tag"

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    mkdir -p "$bin_dir"

    say "downloading $base/$asset"
    curl -fSL --progress-bar -o "$tmp/$asset" "$base/$asset" || err \
        "download failed. Does release $tag exist, with a $triple build?
  https://github.com/$REPO/releases/tag/$tag"
    verify_checksum "$tmp/$asset" "$base/$asset.sha256" "$asset"
    tar -xzf "$tmp/$asset" -C "$bin_dir"
    [ -e "$bin_dir/kbuildx" ] || err "archive did not contain a 'kbuildx' binary"
    chmod +x "$bin_dir/kbuildx"
    say "installed kbuildx ($tag) -> $bin_dir/kbuildx"

    # kbuildx builds inside a bsdkrun microVM everywhere but a Linux host that
    # uses --host, so say where bsdkrun comes from when it is not around.
    if ! command -v bsdkrun >/dev/null 2>&1; then
        say ""
        say "kbuildx drives bsdkrun for its build sandbox. Install it with one of:"
        say "  npm install -g @bsdkrun/cli"
        say "  curl -fsSL https://raw.githubusercontent.com/tsirysndr/bsdkrun/main/install.sh | sh"
    fi

    case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *)
        say ""
        say "add kbuildx to your PATH:"
        say "  export PATH=\"$bin_dir:\$PATH\""
        ;;
    esac
}

# The newest kbuildx-v* release, by the API's publication order. Plain text
# matching rather than a JSON parser, since neither jq nor python can be
# assumed on a fresh machine.
latest_tag() {
    curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=100" 2>/dev/null |
        tr ',' '\n' |
        sed -n 's/^ *"tag_name": *"\(kbuildx-v[^"]*\)".*/\1/p' |
        head -n 1 |
        grep .
}

# Verify against the .sha256 sidecar. A missing sidecar warns and continues;
# a mismatch fails hard.
verify_checksum() {
    archive="$1" sidecar_url="$2" name="$3"
    expected="$(curl -fsSL "$sidecar_url" 2>/dev/null | awk '{print tolower($1)}')" || expected=""
    if [ -z "$expected" ]; then
        say "warning: no checksum sidecar for $name — skipping verification"
        return 0
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$archive" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
    else
        say "warning: no sha256sum/shasum on this system — skipping verification"
        return 0
    fi
    [ "$expected" = "$actual" ] || err "checksum mismatch for $name:
  expected $expected
  actual   $actual"
    say "checksum OK ($(printf '%.12s' "$actual")…)"
}

main "$@"
